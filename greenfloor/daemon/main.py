from __future__ import annotations

import argparse
import asyncio
import concurrent.futures
import contextlib
import fcntl
import json
import logging
import os
import time
from collections import deque
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.price import PriceAdapter, XchPriceProvider
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.io import (
    default_state_dir_path,
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_state_db_path,
)
from greenfloor.config.models import (
    signer_offer_path_configured,
)
from greenfloor.core.cycle import (
    classify_dexie_stale_offer_status,
    collect_stale_sweep_candidates,
    dedupe_sorted_market_ids,
    empty_stale_sweep_payload,
    enqueue_immediate_requeue,
    is_dexie_offer_missing_error_text,
    next_disabled_market_log_deadline,
    record_stale_sweep_check,
    should_use_market_slot_dispatch,
)
from greenfloor.core.cycle import (
    select_market_batch as select_market_batch_kernel,
)
from greenfloor.core.cycle import (
    should_log_disabled_market as should_log_disabled_market_kernel,
)
from greenfloor.core.notifications import utcnow
from greenfloor.daemon.coinset_ws import CoinsetWebsocketClient
from greenfloor.daemon.cooldowns import _env_int
from greenfloor.daemon.inventory_scan import (
    _build_coinset_adapter,
    _resolve_coinset_ws_url,
    _run_coinset_signal_capture_once,
)
from greenfloor.daemon.market_cycle import (
    MarketCycleResult as _MarketCycleResult,  # noqa: F401 — test patch point re-export
)
from greenfloor.daemon.market_cycle import (
    _process_single_market,
    _process_single_market_with_store,
)
from greenfloor.daemon.market_logging import _daemon_logger, _log_market_decision
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.watchlist import _match_watched_coin_ids
from greenfloor.logging_setup import (
    initialize_service_file_logging,
    warn_if_log_level_auto_healed,
)
from greenfloor.storage.sqlite import SqliteStore

_DAEMON_SERVICE_NAME = "daemon"
_DISABLED_MARKET_LOG_INTERVAL_SECONDS_DEFAULT = 3600
_DISABLED_MARKET_NEXT_LOG_AT: dict[str, float] = {}
_DISABLED_MARKET_STARTUP_LOGGED = False
_GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET = 3
_GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS = 60

_DAEMON_INSTANCE_LOCK_FILENAME = "daemon.lock"


def _initialize_daemon_file_logging(home_dir: str, *, log_level: str | None) -> None:
    initialize_service_file_logging(
        service_name=_DAEMON_SERVICE_NAME,
        home_dir=home_dir,
        log_level=log_level,
        service_logger=_daemon_logger,
        allow_reinit_level=True,
    )


def _disabled_market_log_interval_seconds() -> int:
    return _env_int(
        "GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS",
        _DISABLED_MARKET_LOG_INTERVAL_SECONDS_DEFAULT,
        minimum=60,
    )


def _should_log_disabled_market(*, market_id: str, now_monotonic: float | None = None) -> bool:
    now_value = time.monotonic() if now_monotonic is None else float(now_monotonic)
    deadline = float(_DISABLED_MARKET_NEXT_LOG_AT.get(market_id, 0.0))
    if not should_log_disabled_market_kernel(
        now_monotonic=now_value,
        next_log_deadline=deadline,
    ):
        return False
    _DISABLED_MARKET_NEXT_LOG_AT[market_id] = next_disabled_market_log_deadline(
        now_monotonic=now_value,
        interval_seconds=_disabled_market_log_interval_seconds(),
    )
    return True


def _log_disabled_markets_startup_once(*, markets: list[Any]) -> None:
    global _DISABLED_MARKET_STARTUP_LOGGED
    if _DISABLED_MARKET_STARTUP_LOGGED:
        return
    interval_seconds = _disabled_market_log_interval_seconds()
    disabled_market_ids = [
        str(getattr(market, "market_id", "")).strip()
        for market in markets
        if not bool(getattr(market, "enabled", True))
    ]
    disabled_market_ids = [market_id for market_id in disabled_market_ids if market_id]
    if disabled_market_ids:
        _daemon_logger.info(
            "disabled_markets_startup count=%s interval_seconds=%s market_ids=%s",
            len(disabled_market_ids),
            interval_seconds,
            sorted(disabled_market_ids),
        )
        now_value = time.monotonic()
        for market_id in disabled_market_ids:
            _DISABLED_MARKET_NEXT_LOG_AT[market_id] = now_value + float(interval_seconds)
    _DISABLED_MARKET_STARTUP_LOGGED = True


def _warn_if_log_level_auto_healed(*, program, program_path: Path) -> None:
    warn_if_log_level_auto_healed(
        program_obj=program, program_path=program_path, logger=_daemon_logger
    )


def _log_daemon_event(*, level: int, payload: dict[str, Any]) -> None:
    _daemon_logger.log(level, "daemon_event %s", json.dumps(payload, sort_keys=True))


def _consume_reload_marker(state_dir: Path) -> bool:
    marker = state_dir / "reload_request.json"
    if not marker.exists():
        return False
    marker.unlink(missing_ok=True)
    return True


def _daemon_instance_lock_path(*, state_dir: Path) -> Path:
    return state_dir / _DAEMON_INSTANCE_LOCK_FILENAME


@contextlib.contextmanager
def _acquire_daemon_instance_lock(*, state_dir: Path, mode: str):
    state_dir.mkdir(parents=True, exist_ok=True)
    lock_path = _daemon_instance_lock_path(state_dir=state_dir)
    lock_file = lock_path.open("a+", encoding="utf-8")
    try:
        try:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            existing = ""
            try:
                lock_file.seek(0)
                existing = lock_file.read().strip()
            except Exception:
                existing = ""
            detail = f" daemon_lock_metadata={existing}" if existing else ""
            raise RuntimeError(f"daemon_already_running:{lock_path}{detail}") from exc
        payload = {
            "pid": os.getpid(),
            "mode": str(mode).strip(),
            "acquired_at": datetime.now(UTC).isoformat(),
        }
        lock_file.seek(0)
        lock_file.truncate()
        lock_file.write(json.dumps(payload, sort_keys=True))
        lock_file.flush()
        yield
    finally:
        try:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)
        except Exception:
            pass
        lock_file.close()


@dataclass(slots=True)
class _MarketDispatchState:
    cursor: int = 0
    immediate_requeue_ids: deque[str] = field(default_factory=deque)


def _enqueue_immediate_requeue_market(dispatch_state: _MarketDispatchState, market_id: str) -> None:
    dispatch_state.immediate_requeue_ids = deque(
        enqueue_immediate_requeue(list(dispatch_state.immediate_requeue_ids), market_id)
    )


def _select_market_batch(
    *,
    enabled_markets: list[Any],
    slot_count: int,
    dispatch_state: _MarketDispatchState,
) -> tuple[list[Any], list[str]]:
    enabled_by_id: dict[str, Any] = {
        str(getattr(market, "market_id", "")).strip(): market for market in enabled_markets
    }
    enabled_ids = [market_id for market_id in enabled_by_id if market_id]
    if not enabled_ids:
        dispatch_state.immediate_requeue_ids = deque()
        dispatch_state.cursor = 0
        return [], []

    selection = select_market_batch_kernel(
        enabled_market_ids=enabled_ids,
        slot_count=int(slot_count),
        cursor=int(dispatch_state.cursor),
        immediate_requeue_ids=list(dispatch_state.immediate_requeue_ids),
    )
    dispatch_state.cursor = int(selection.get("cursor", dispatch_state.cursor))
    dispatch_state.immediate_requeue_ids = deque(
        str(market_id)
        for market_id in selection.get("immediate_requeue_ids", [])
        if str(market_id).strip()
    )
    selected_markets = [
        enabled_by_id[str(market_id)]
        for market_id in selection.get("selected_market_ids", [])
        if str(market_id).strip() in enabled_by_id
    ]
    consumed = [
        str(market_id)
        for market_id in selection.get("consumed_immediate_requeues", [])
        if str(market_id).strip()
    ]
    return selected_markets, consumed


def _detect_stale_open_offers_for_requeue(
    *,
    store: SqliteStore,
    dexie: DexieAdapter,
    enabled_market_ids: set[str],
    per_market_limit: int = _GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET,
    max_offer_checks: int = _GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS,
) -> dict[str, Any]:
    if not enabled_market_ids:
        return empty_stale_sweep_payload()

    rows = store.list_offer_states(limit=5000)
    offer_rows = [
        {
            "market_id": str(row.get("market_id", "")),
            "offer_id": str(row.get("offer_id", "")),
            "state": str(row.get("state", "")),
        }
        for row in rows
    ]
    candidates = collect_stale_sweep_candidates(
        rows=offer_rows,
        enabled_market_ids=sorted(enabled_market_ids),
        per_market_limit=max(1, int(per_market_limit)),
    )
    progress = empty_stale_sweep_payload()
    check_limit = max(1, int(max_offer_checks))
    for candidate in candidates:
        if int(progress["checked_offer_count"]) >= check_limit:
            progress["truncated"] = True
            return progress
        market_id = str(candidate.get("market_id", "")).strip()
        offer_id = str(candidate.get("offer_id", "")).strip()
        hit: dict[str, str] | None = None
        try:
            payload = dexie.get_offer(offer_id, timeout=5)
            offer = payload.get("offer") if isinstance(payload, dict) else None
            if isinstance(offer, dict):
                try:
                    status = int(offer.get("status", -1))
                except (TypeError, ValueError):
                    status = -1
                reason = classify_dexie_stale_offer_status(status)
                if reason:
                    hit = {
                        "market_id": market_id,
                        "offer_id": offer_id,
                        "reason": reason,
                    }
        except Exception as exc:  # pragma: no cover - network dependent
            if is_dexie_offer_missing_error_text(str(exc)):
                hit = {
                    "market_id": market_id,
                    "offer_id": offer_id,
                    "reason": "offer_missing_404",
                }
        progress = record_stale_sweep_check(progress=progress, hit=hit)
    return progress


def run_once(
    program_path: Path,
    markets_path: Path,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool = True,
    use_websocket_capture: bool = False,
    program=None,
    testnet_markets_path: Path | None = None,
    market_dispatch_state: _MarketDispatchState | None = None,
) -> int:
    if program is None:
        program = load_program_config(program_path)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    _log_disabled_markets_startup_once(markets=list(markets.markets))
    db_path = resolve_state_db_path(
        program_home_dir=program.home_dir,
        explicit_db_path=db_path_override,
    )
    store = SqliteStore(db_path)
    started_at = time.monotonic()

    try:
        markets_processed = 0
        markets_attempted = 0
        cycle_error_count = 0
        strategy_planned_total = 0
        strategy_executed_total = 0
        cancel_triggered_count = 0
        cancel_planned_total = 0
        cancel_executed_total = 0
        dexie = DexieAdapter(program.dexie_api_base)
        splash = SplashAdapter(program.splash_api_base)
        wallet = WalletAdapter()
        price = XchPriceProvider(fallback_price_adapter=PriceAdapter())
        previous_xch_price_usd = store.get_latest_xch_price_snapshot()
        reservation_coordinator: AssetReservationCoordinator | None = None
        if bool(
            getattr(program, "runtime_offer_parallelism_enabled", False)
        ) and signer_offer_path_configured(program):
            reservation_coordinator = AssetReservationCoordinator(
                db_path=db_path,
                lease_seconds=int(getattr(program, "runtime_reservation_ttl_seconds", 300)),
            )
            expired_count = reservation_coordinator.expire_stale()
            if expired_count > 0:
                store.add_audit_event("reservation_expired", {"count": int(expired_count)})
        xch_price_usd: float | None = None
        try:
            xch_price_usd = asyncio.run(price.get_xch_price())
            store.add_audit_event("xch_price_snapshot", {"price_usd": xch_price_usd})
        except Exception as exc:  # pragma: no cover - network dependent
            cycle_error_count += 1
            store.add_audit_event("xch_price_error", {"error": str(exc)})
        if use_websocket_capture:
            try:
                _run_coinset_signal_capture_once(
                    program=program,
                    coinset_base_url=coinset_base_url,
                    store=store,
                )
            except Exception as exc:  # pragma: no cover - network dependent
                cycle_error_count += 1
                store.add_audit_event("coinset_ws_once_error", {"error": str(exc)})
        elif poll_coinset_mempool:
            try:
                coinset = _build_coinset_adapter(program=program, coinset_base_url=coinset_base_url)
                tx_ids = coinset.get_all_mempool_tx_ids()
                new_count = store.observe_mempool_tx_ids(tx_ids)
                store.add_audit_event("coinset_mempool_snapshot", {"count": len(tx_ids)})
                if new_count:
                    store.add_audit_event("mempool_observed", {"new_tx_ids": new_count})
            except Exception as exc:  # pragma: no cover - network dependent
                cycle_error_count += 1
                store.add_audit_event("coinset_mempool_error", {"error": str(exc)})

        now = utcnow()
        enabled_markets: list[Any] = []
        for market in markets.markets:
            if not market.enabled:
                if _should_log_disabled_market(market_id=market.market_id):
                    _log_market_decision(market.market_id, "market_skipped", reason="disabled")
                continue
            _DISABLED_MARKET_NEXT_LOG_AT.pop(market.market_id, None)
            enabled_markets.append(market)

        stale_open_sweep_payload: dict[str, Any] = {
            "checked_offer_count": 0,
            "requeue_market_ids": [],
            "hits": [],
            "truncated": False,
        }
        if enabled_markets:
            stale_open_sweep_payload = _detect_stale_open_offers_for_requeue(
                store=store,
                dexie=dexie,
                enabled_market_ids={
                    str(getattr(market, "market_id", "")).strip() for market in enabled_markets
                },
            )
            stale_requeues = [
                str(mid).strip()
                for mid in stale_open_sweep_payload.get("requeue_market_ids", [])
                if str(mid).strip()
            ]
            if market_dispatch_state is not None:
                for market_id in stale_requeues:
                    _enqueue_immediate_requeue_market(market_dispatch_state, market_id)
            if stale_requeues:
                store.add_audit_event(
                    "stale_open_offer_requeue_detected",
                    {
                        "market_ids": stale_requeues,
                        "checked_offer_count": int(
                            stale_open_sweep_payload.get("checked_offer_count", 0)
                        ),
                        "truncated": bool(stale_open_sweep_payload.get("truncated", False)),
                        "hits": list(stale_open_sweep_payload.get("hits", []))[:50],
                    },
                )

        configured_market_slot_count = int(getattr(program, "runtime_market_slot_count", 0))
        consumed_immediate_requeues: list[str] = []
        if market_dispatch_state is not None and should_use_market_slot_dispatch(
            enabled_market_count=len(enabled_markets),
            slot_count=configured_market_slot_count,
        ):
            selected_markets, consumed_immediate_requeues = _select_market_batch(
                enabled_markets=enabled_markets,
                slot_count=configured_market_slot_count,
                dispatch_state=market_dispatch_state,
            )
            _daemon_logger.info(
                "market_slot_dispatch enabled=true slot_count=%s selected=%s enabled=%s immediate_requeue_consumed=%s cursor=%s pending_requeues=%s",
                configured_market_slot_count,
                len(selected_markets),
                len(enabled_markets),
                len(consumed_immediate_requeues),
                market_dispatch_state.cursor,
                len(market_dispatch_state.immediate_requeue_ids),
            )
        else:
            selected_markets = enabled_markets
            if market_dispatch_state is not None and enabled_markets:
                # Keep scheduler cursor bounded even when slot dispatch is disabled.
                market_dispatch_state.cursor %= len(enabled_markets)
        markets_attempted = len(selected_markets)
        immediate_requeue_market_ids: list[str] = []
        if bool(getattr(program, "runtime_parallel_markets", False)) and len(selected_markets) > 1:
            max_workers = max(1, len(selected_markets))
            _daemon_logger.info(
                "market_parallel_dispatch enabled=true workers=%s markets=%s",
                max_workers,
                markets_attempted,
            )
            with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
                future_to_market = {
                    pool.submit(
                        _process_single_market_with_store,
                        market=market,
                        program=program,
                        allowed_keys=allowed_keys,
                        dexie=dexie,
                        splash=splash,
                        wallet=wallet,
                        db_path=db_path,
                        xch_price_usd=xch_price_usd,
                        previous_xch_price_usd=previous_xch_price_usd,
                        now=now,
                        state_dir=state_dir,
                        reservation_coordinator=reservation_coordinator,
                    ): market
                    for market in selected_markets
                }
                for future in concurrent.futures.as_completed(future_to_market):
                    market = future_to_market[future]
                    market_id = str(getattr(market, "market_id", "")).strip()
                    try:
                        mr = future.result()
                    except Exception as exc:
                        cycle_error_count += 1
                        _log_market_decision(
                            market_id or "unknown",
                            "cycle_failed",
                            error=str(exc),
                        )
                        # This runs in the main thread while iterating
                        # `as_completed`, so emitting the aggregate market-cycle
                        # error through the outer store is thread-safe.
                        store.add_audit_event(
                            "market_cycle_error",
                            {
                                "market_id": market_id,
                                "error": str(exc),
                                "source": "parallel_market_worker",
                            },
                        )
                        continue
                    markets_processed += 1
                    cycle_error_count += mr.cycle_errors
                    strategy_planned_total += mr.strategy_planned
                    strategy_executed_total += mr.strategy_executed
                    if mr.cancel_triggered:
                        cancel_triggered_count += 1
                    cancel_planned_total += mr.cancel_planned
                    cancel_executed_total += mr.cancel_executed
                    if mr.immediate_requeue_requested and market_id:
                        immediate_requeue_market_ids.append(market_id)
        else:
            _daemon_logger.info(
                "market_parallel_dispatch enabled=false workers=1 markets=%s",
                markets_attempted,
            )
            for market in selected_markets:
                market_id = str(getattr(market, "market_id", "")).strip()
                try:
                    mr = _process_single_market(
                        market=market,
                        program=program,
                        allowed_keys=allowed_keys,
                        dexie=dexie,
                        splash=splash,
                        wallet=wallet,
                        store=store,
                        xch_price_usd=xch_price_usd,
                        previous_xch_price_usd=previous_xch_price_usd,
                        now=now,
                        state_dir=state_dir,
                        reservation_coordinator=reservation_coordinator,
                    )
                except Exception as exc:
                    cycle_error_count += 1
                    _log_market_decision(
                        market_id or "unknown",
                        "cycle_failed",
                        error=str(exc),
                    )
                    store.add_audit_event(
                        "market_cycle_error",
                        {
                            "market_id": market_id,
                            "error": str(exc),
                            "source": "sequential_market_worker",
                        },
                    )
                    continue
                markets_processed += 1
                cycle_error_count += mr.cycle_errors
                strategy_planned_total += mr.strategy_planned
                strategy_executed_total += mr.strategy_executed
                if mr.cancel_triggered:
                    cancel_triggered_count += 1
                cancel_planned_total += mr.cancel_planned
                cancel_executed_total += mr.cancel_executed
                if mr.immediate_requeue_requested and market_id:
                    immediate_requeue_market_ids.append(market_id)
        deduped_requeue_market_ids = dedupe_sorted_market_ids(immediate_requeue_market_ids)
        if market_dispatch_state is not None:
            for market_id in deduped_requeue_market_ids:
                _enqueue_immediate_requeue_market(market_dispatch_state, market_id)
        duration_ms = int((time.monotonic() - started_at) * 1000)
        store.add_audit_event(
            "daemon_cycle_summary",
            {
                "duration_ms": duration_ms,
                "enabled_markets": len(enabled_markets),
                "markets_attempted": markets_attempted,
                "markets_processed": markets_processed,
                "runtime_market_slot_count": configured_market_slot_count,
                "stale_open_sweep_checked_offer_count": int(
                    stale_open_sweep_payload.get("checked_offer_count", 0)
                ),
                "stale_open_sweep_requeue_market_ids": list(
                    stale_open_sweep_payload.get("requeue_market_ids", [])
                ),
                "stale_open_sweep_requeue_count": len(
                    list(stale_open_sweep_payload.get("requeue_market_ids", []))
                ),
                "stale_open_sweep_truncated": bool(
                    stale_open_sweep_payload.get("truncated", False)
                ),
                "immediate_requeue_market_ids": deduped_requeue_market_ids,
                "immediate_requeue_count": len(deduped_requeue_market_ids),
                "error_count": cycle_error_count,
                "strategy_planned_total": strategy_planned_total,
                "strategy_executed_total": strategy_executed_total,
                "cancel_triggered_count": cancel_triggered_count,
                "cancel_planned_total": cancel_planned_total,
                "cancel_executed_total": cancel_executed_total,
            },
        )
        return 0
    finally:
        store.close()


def _run_loop(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
) -> int:
    current_program = load_program_config(program_path)
    market_dispatch_state = _MarketDispatchState()
    _initialize_daemon_file_logging(
        current_program.home_dir, log_level=getattr(current_program, "app_log_level", "INFO")
    )
    _warn_if_log_level_auto_healed(program=current_program, program_path=program_path)
    _daemon_logger.info(
        "daemon_starting mode=loop program_config=%s markets_config=%s",
        os.fspath(program_path),
        os.fspath(markets_path),
    )
    db_path = resolve_state_db_path(
        program_home_dir=current_program.home_dir,
        explicit_db_path=db_path_override,
    )
    coinset = _build_coinset_adapter(program=current_program, coinset_base_url=coinset_base_url)
    ws_url = _resolve_coinset_ws_url(program=current_program, coinset_base_url=coinset_base_url)

    def _with_ws_store(callback: Callable[[SqliteStore], None]) -> None:
        # Websocket callbacks may run on a worker thread, so open a
        # callback-local SQLite connection instead of reusing a main-thread store.
        store = SqliteStore(db_path)
        try:
            callback(store)
        finally:
            store.close()

    def _on_mempool_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return

        def _write(store: SqliteStore) -> None:
            new_count = store.observe_mempool_tx_ids(tx_ids)
            if new_count:
                store.add_audit_event(
                    "mempool_observed",
                    {"new_tx_ids": new_count, "source": "coinset_websocket"},
                )

        _with_ws_store(_write)

    def _on_confirmed_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return

        def _write(store: SqliteStore) -> None:
            confirmed = store.confirm_tx_ids(tx_ids)
            store.add_audit_event(
                "tx_block_confirmed",
                {
                    "tx_ids": tx_ids,
                    "confirmed_count": confirmed,
                    "source": "coinset_websocket",
                },
            )

        _with_ws_store(_write)

    def _on_audit_event(event_type: str, payload: dict[str, Any]) -> None:
        _with_ws_store(lambda store: store.add_audit_event(event_type, payload))

    def _on_observed_coin_ids(coin_ids: list[str]) -> None:
        if not coin_ids:
            return
        hits = _match_watched_coin_ids(observed_coin_ids=coin_ids)
        if not hits:
            return

        def _write(store: SqliteStore) -> None:
            store.add_audit_event(
                "coin_watch_hit",
                {
                    "coin_id_count": len(coin_ids),
                    "coin_ids_sample": sorted({str(c).strip().lower() for c in coin_ids})[:10],
                    "market_hits": {market_id: ids[:10] for market_id, ids in hits.items()},
                    "source": "coinset_websocket",
                },
            )

        _with_ws_store(_write)

    ws_client = CoinsetWebsocketClient(
        ws_url=ws_url,
        reconnect_interval_seconds=current_program.tx_block_websocket_reconnect_interval_seconds,
        on_mempool_tx_ids=_on_mempool_tx_ids,
        on_confirmed_tx_ids=_on_confirmed_tx_ids,
        on_audit_event=_on_audit_event,
        on_observed_coin_ids=_on_observed_coin_ids,
        recovery_poll=coinset.get_all_mempool_tx_ids,
    )
    ws_client.start()

    try:
        while True:
            _initialize_daemon_file_logging(
                current_program.home_dir,
                log_level=getattr(current_program, "app_log_level", "INFO"),
            )
            _warn_if_log_level_auto_healed(program=current_program, program_path=program_path)
            run_once(
                program_path=program_path,
                markets_path=markets_path,
                testnet_markets_path=testnet_markets_path,
                allowed_keys=allowed_keys,
                db_path_override=db_path_override,
                coinset_base_url=coinset_base_url,
                state_dir=state_dir,
                poll_coinset_mempool=False,
                program=current_program,
                market_dispatch_state=market_dispatch_state,
            )
            if _consume_reload_marker(state_dir):
                _log_daemon_event(level=logging.INFO, payload={"event": "config_reloaded"})
            time.sleep(max(1, current_program.runtime_loop_interval_seconds))
            current_program = load_program_config(program_path)
    except KeyboardInterrupt:
        return 0
    finally:
        ws_client.stop()
        _daemon_logger.info("daemon_stopped mode=loop")


def main() -> None:
    def _default_testnet_markets_config_path() -> str:
        candidate = Path("~/.greenfloor/config/testnet-markets.yaml").expanduser()
        if candidate.exists():
            return str(candidate)
        return ""

    parser = argparse.ArgumentParser(description="Run GreenFloor daemon")
    parser.add_argument(
        "--program-config",
        default="config/program.yaml",
        help="Path to program.yaml",
    )
    parser.add_argument(
        "--markets-config",
        default="config/markets.yaml",
        help="Path to markets.yaml",
    )
    parser.add_argument(
        "--testnet-markets-config",
        default=_default_testnet_markets_config_path(),
        help=(
            "Optional path to testnet-markets.yaml overlay. "
            "Ignored when unset or file does not exist."
        ),
    )
    parser.add_argument(
        "--key-ids",
        default="",
        help="Comma-separated signer key IDs allowed for this daemon instance",
    )
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run one evaluation cycle and exit",
    )
    parser.add_argument("--state-db", default="", help="Optional explicit SQLite state DB path")
    parser.add_argument(
        "--coinset-base-url",
        default="https://api.coinset.org",
        help="Coinset API base URL",
    )
    parser.add_argument(
        "--state-dir",
        default=str(default_state_dir_path()),
        help="State directory used for reload marker and daemon-local state",
    )
    args = parser.parse_args()
    state_dir = Path(args.state_dir).expanduser()
    testnet_markets_path = (
        Path(args.testnet_markets_config) if str(args.testnet_markets_config).strip() else None
    )

    allowed_keys = {k.strip() for k in args.key_ids.split(",") if k.strip()} or None
    try:
        with _acquire_daemon_instance_lock(
            state_dir=state_dir,
            mode="once" if args.once else "loop",
        ):
            if args.once:
                program = load_program_config(Path(args.program_config))
                _initialize_daemon_file_logging(
                    program.home_dir, log_level=getattr(program, "app_log_level", "INFO")
                )
                _warn_if_log_level_auto_healed(
                    program=program, program_path=Path(args.program_config)
                )
                _daemon_logger.info(
                    "daemon_starting mode=once program_config=%s markets_config=%s",
                    args.program_config,
                    args.markets_config,
                )
                exit_code = run_once(
                    Path(args.program_config),
                    Path(args.markets_config),
                    allowed_keys,
                    args.state_db or None,
                    args.coinset_base_url,
                    state_dir,
                    poll_coinset_mempool=False,
                    use_websocket_capture=program.tx_block_trigger_mode == "websocket",
                    testnet_markets_path=testnet_markets_path,
                )
                _daemon_logger.info("daemon_stopped mode=once exit_code=%s", exit_code)
            else:
                exit_code = _run_loop(
                    program_path=Path(args.program_config),
                    markets_path=Path(args.markets_config),
                    testnet_markets_path=testnet_markets_path,
                    allowed_keys=allowed_keys,
                    db_path_override=args.state_db or None,
                    coinset_base_url=args.coinset_base_url,
                    state_dir=state_dir,
                )
    except RuntimeError as exc:
        try:
            program = load_program_config(Path(args.program_config))
            _initialize_daemon_file_logging(
                program.home_dir, log_level=getattr(program, "app_log_level", "INFO")
            )
            _warn_if_log_level_auto_healed(program=program, program_path=Path(args.program_config))
        except Exception:
            pass
        _log_daemon_event(
            level=logging.ERROR,
            payload={"event": "daemon_lock_conflict", "error": str(exc)},
        )
        raise SystemExit(3) from exc
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()
