"""Daemon cycle orchestration: single-cycle execution and the long-running loop."""

from __future__ import annotations

import asyncio
import logging
import os
import time
from dataclasses import asdict
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.price import PriceAdapter, XchPriceProvider
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_state_db_path,
)
from greenfloor.config.models import signer_offer_path_configured
from greenfloor.core.cycle import (
    dedupe_sorted_market_ids,
    empty_stale_sweep_payload,
    should_use_market_slot_dispatch,
)
from greenfloor.core.cycle_orchestration import StaleSweepProgress
from greenfloor.core.notifications import utcnow
from greenfloor.daemon.bootstrap import (
    initialize_daemon_file_logging,
    log_daemon_event,
    warn_if_daemon_log_level_auto_healed,
)
from greenfloor.daemon.coinset_ws import CoinsetWebsocketClient
from greenfloor.daemon.cycle_market_batch import (
    MarketDispatchState,
    clear_disabled_market_log_state,
    enqueue_immediate_requeue_market,
    log_disabled_market_skip,
    log_disabled_markets_startup_once,
    select_market_batch,
    should_log_disabled_market,
)
from greenfloor.daemon.cycle_market_dispatch import dispatch_selected_markets
from greenfloor.daemon.cycle_stale_sweep import detect_stale_open_offers_for_requeue
from greenfloor.daemon.cycle_ws_handlers import build_coinset_websocket_handlers
from greenfloor.daemon.inventory_scan import (
    _build_coinset_adapter,
    _resolve_coinset_ws_url,
    _run_coinset_signal_capture_once,
)

# Backward-compatible re-exports for tests and legacy imports.
from greenfloor.daemon.market_cycle import (  # noqa: F401
    process_single_market,
    process_single_market_with_store,
)
from greenfloor.daemon.market_logging import _daemon_logger
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.storage.sqlite import SqliteStore

# Backward-compatible aliases for tests and legacy imports.
_should_log_disabled_market = should_log_disabled_market
_log_disabled_markets_startup_once = log_disabled_markets_startup_once


def consume_reload_marker(state_dir: Path) -> bool:
    marker = state_dir / "reload_request.json"
    if not marker.exists():
        return False
    marker.unlink(missing_ok=True)
    return True


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
    market_dispatch_state: MarketDispatchState | None = None,
) -> int:
    if program is None:
        program = load_program_config(program_path)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    log_disabled_markets_startup_once(markets=list(markets.markets))
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
                log_disabled_market_skip(market_id=market.market_id)
                continue
            clear_disabled_market_log_state(market_id=market.market_id)
            enabled_markets.append(market)

        stale_open_sweep_payload: StaleSweepProgress = empty_stale_sweep_payload()
        if enabled_markets:
            stale_open_sweep_payload = detect_stale_open_offers_for_requeue(
                store=store,
                dexie=dexie,
                enabled_market_ids={
                    str(getattr(market, "market_id", "")).strip() for market in enabled_markets
                },
            )
            stale_requeues = [
                str(mid).strip()
                for mid in stale_open_sweep_payload.requeue_market_ids
                if str(mid).strip()
            ]
            if market_dispatch_state is not None:
                for market_id in stale_requeues:
                    enqueue_immediate_requeue_market(market_dispatch_state, market_id)
            if stale_requeues:
                store.add_audit_event(
                    "stale_open_offer_requeue_detected",
                    {
                        "market_ids": stale_requeues,
                        "checked_offer_count": int(stale_open_sweep_payload.checked_offer_count),
                        "truncated": bool(stale_open_sweep_payload.truncated),
                        "hits": [asdict(hit) for hit in stale_open_sweep_payload.hits][:50],
                    },
                )

        configured_market_slot_count = int(getattr(program, "runtime_market_slot_count", 0))
        consumed_immediate_requeues: list[str] = []
        if market_dispatch_state is not None and should_use_market_slot_dispatch(
            enabled_market_count=len(enabled_markets),
            slot_count=configured_market_slot_count,
        ):
            selected_markets, consumed_immediate_requeues = select_market_batch(
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
                market_dispatch_state.cursor %= len(enabled_markets)
        markets_attempted = len(selected_markets)
        parallel_markets_enabled = bool(getattr(program, "runtime_parallel_markets", False))
        _daemon_logger.info(
            "market_parallel_dispatch enabled=%s workers=%s markets=%s",
            parallel_markets_enabled and markets_attempted > 1,
            max(1, markets_attempted) if parallel_markets_enabled and markets_attempted > 1 else 1,
            markets_attempted,
        )
        dispatch_result = dispatch_selected_markets(
            program=program,
            selected_markets=selected_markets,
            allowed_keys=allowed_keys,
            dexie=dexie,
            splash=splash,
            wallet=wallet,
            store=store,
            db_path=db_path,
            xch_price_usd=xch_price_usd,
            previous_xch_price_usd=previous_xch_price_usd,
            now=now,
            state_dir=state_dir,
            reservation_coordinator=reservation_coordinator,
            parallel_markets_enabled=parallel_markets_enabled,
        )
        markets_processed = dispatch_result.markets_processed
        cycle_error_count += dispatch_result.cycle_error_count
        strategy_planned_total += dispatch_result.strategy_planned_total
        strategy_executed_total += dispatch_result.strategy_executed_total
        cancel_triggered_count += dispatch_result.cancel_triggered_count
        cancel_planned_total += dispatch_result.cancel_planned_total
        cancel_executed_total += dispatch_result.cancel_executed_total
        deduped_requeue_market_ids = dedupe_sorted_market_ids(
            dispatch_result.immediate_requeue_market_ids
        )
        if market_dispatch_state is not None:
            for market_id in deduped_requeue_market_ids:
                enqueue_immediate_requeue_market(market_dispatch_state, market_id)
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
                    stale_open_sweep_payload.checked_offer_count
                ),
                "stale_open_sweep_requeue_market_ids": list(
                    stale_open_sweep_payload.requeue_market_ids
                ),
                "stale_open_sweep_requeue_count": len(stale_open_sweep_payload.requeue_market_ids),
                "stale_open_sweep_truncated": bool(stale_open_sweep_payload.truncated),
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


def run_loop(
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
    market_dispatch_state = MarketDispatchState()
    initialize_daemon_file_logging(
        current_program.home_dir, log_level=getattr(current_program, "app_log_level", "INFO")
    )
    warn_if_daemon_log_level_auto_healed(program=current_program, program_path=program_path)
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
    ws_handlers = build_coinset_websocket_handlers(db_path=db_path)

    ws_client = CoinsetWebsocketClient(
        ws_url=ws_url,
        reconnect_interval_seconds=current_program.tx_block_websocket_reconnect_interval_seconds,
        on_mempool_tx_ids=ws_handlers.on_mempool_tx_ids,
        on_confirmed_tx_ids=ws_handlers.on_confirmed_tx_ids,
        on_audit_event=ws_handlers.on_audit_event,
        on_observed_coin_ids=ws_handlers.on_observed_coin_ids,
        recovery_poll=coinset.get_all_mempool_tx_ids,
    )
    ws_client.start()

    try:
        while True:
            initialize_daemon_file_logging(
                current_program.home_dir,
                log_level=getattr(current_program, "app_log_level", "INFO"),
            )
            warn_if_daemon_log_level_auto_healed(program=current_program, program_path=program_path)
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
            if consume_reload_marker(state_dir):
                log_daemon_event(level=logging.INFO, payload={"event": "config_reloaded"})
            time.sleep(max(1, current_program.runtime_loop_interval_seconds))
            current_program = load_program_config(program_path)
    except KeyboardInterrupt:
        return 0
    finally:
        ws_client.stop()
        _daemon_logger.info("daemon_stopped mode=loop")
