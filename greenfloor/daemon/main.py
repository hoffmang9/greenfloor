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
    ProgramConfig,
    signer_offer_path_configured,
)
from greenfloor.core.inventory import compute_bucket_counts_from_coins
from greenfloor.core.notifications import AlertState, evaluate_low_inventory_alert, utcnow
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.core.strategy import evaluate_market
from greenfloor.daemon.cancel_policy import _execute_cancel_policy_for_market
from greenfloor.daemon.coin_ops_cycle import (
    _executed_sell_offer_counts_by_size,
    _plan_and_execute_coin_ops,
)
from greenfloor.daemon.coinset_ws import CoinsetWebsocketClient
from greenfloor.daemon.cooldowns import (
    _env_int,
    _managed_offer_market_health_payload,
)
from greenfloor.daemon.inventory_scan import (
    _build_coinset_adapter,
    _coinset_cat_spendable_base_unit_coin_amounts,
    _coinset_spendable_base_unit_coin_amounts,
    _resolve_coinset_ws_url,
    _run_coinset_signal_capture_once,
)
from greenfloor.daemon.market_helpers import (
    _base_unit_mojo_multiplier_for_market,
    _normalize_offer_side,
)
from greenfloor.daemon.market_logging import (
    _daemon_logger,
    _log_market_decision,
)
from greenfloor.daemon.offer_reconcile_cycle import reconcile_market_cycle_offers
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_dispatch import (
    _execute_strategy_actions,
    _resolve_signer_offer_asset_ids_for_reservation,
)
from greenfloor.daemon.strategy_reseed import _inject_reseed_action_if_no_active_offers
from greenfloor.daemon.strategy_state import (
    _evaluate_two_sided_market_actions,
    _strategy_config_from_market,
    _strategy_state_from_bucket_counts,
)
from greenfloor.daemon.watchlist import (
    _active_offer_counts_by_size,
    _active_offer_counts_by_size_and_side,
    _is_dexie_offer_missing_error,
    _match_watched_coin_ids,
    _strategy_target_counts_by_size,
)
from greenfloor.keys.router import resolve_market_key
from greenfloor.logging_setup import (
    initialize_service_file_logging,
    warn_if_log_level_auto_healed,
)
from greenfloor.notify.pushover import send_pushover_alert
from greenfloor.storage.sqlite import SqliteStore, StoredAlertState

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
    if deadline > now_value:
        return False
    _DISABLED_MARKET_NEXT_LOG_AT[market_id] = now_value + float(
        _disabled_market_log_interval_seconds()
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
class _MarketCycleResult:
    cycle_errors: int = 0
    strategy_planned: int = 0
    strategy_executed: int = 0
    cancel_triggered: bool = False
    cancel_planned: int = 0
    cancel_executed: int = 0
    immediate_requeue_requested: bool = False
    immediate_requeue_signals: list[str] = field(default_factory=list)


@dataclass(slots=True)
class _MarketDispatchState:
    cursor: int = 0
    immediate_requeue_ids: deque[str] = field(default_factory=deque)


def _enqueue_immediate_requeue_market(dispatch_state: _MarketDispatchState, market_id: str) -> None:
    clean_market_id = str(market_id).strip()
    if not clean_market_id:
        return
    deduped_existing = deque(
        mid for mid in dispatch_state.immediate_requeue_ids if mid != clean_market_id
    )
    deduped_existing.appendleft(clean_market_id)
    dispatch_state.immediate_requeue_ids = deduped_existing


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

    max_slots = max(1, int(slot_count))
    if max_slots >= len(enabled_ids):
        # Keep only currently enabled markets in the requeue deque.
        dispatch_state.immediate_requeue_ids = deque(
            mid for mid in dispatch_state.immediate_requeue_ids if mid in enabled_by_id
        )
        return [enabled_by_id[mid] for mid in enabled_ids], []

    selected_ids: list[str] = []
    selected_set: set[str] = set()
    retained_requeues: deque[str] = deque()
    consumed_requeues: list[str] = []
    for market_id in list(dispatch_state.immediate_requeue_ids):
        if market_id not in enabled_by_id:
            continue
        if market_id in selected_set:
            continue
        if len(selected_ids) < max_slots:
            selected_ids.append(market_id)
            selected_set.add(market_id)
            consumed_requeues.append(market_id)
        else:
            retained_requeues.append(market_id)
    dispatch_state.immediate_requeue_ids = retained_requeues

    round_robin_slots = max_slots - len(selected_ids)
    if round_robin_slots > 0:
        total_enabled = len(enabled_ids)
        start_idx = dispatch_state.cursor % total_enabled
        last_rr_idx: int | None = None
        for step in range(total_enabled):
            idx = (start_idx + step) % total_enabled
            market_id = enabled_ids[idx]
            if market_id in selected_set:
                continue
            selected_ids.append(market_id)
            selected_set.add(market_id)
            last_rr_idx = idx
            if len(selected_ids) >= max_slots:
                break
        if last_rr_idx is not None:
            dispatch_state.cursor = (last_rr_idx + 1) % total_enabled

    selected_markets = [
        enabled_by_id[market_id] for market_id in selected_ids if market_id in enabled_by_id
    ]
    return selected_markets, consumed_requeues


def _detect_stale_open_offers_for_requeue(
    *,
    store: SqliteStore,
    dexie: DexieAdapter,
    enabled_market_ids: set[str],
    per_market_limit: int = _GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET,
    max_offer_checks: int = _GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS,
) -> dict[str, Any]:
    if not enabled_market_ids:
        return {
            "checked_offer_count": 0,
            "requeue_market_ids": [],
            "hits": [],
        }
    rows = store.list_offer_states(limit=5000)
    tracked_states = {
        OfferLifecycleState.OPEN.value,
        OfferLifecycleState.REFRESH_DUE.value,
    }
    offer_ids_by_market: dict[str, list[str]] = {}
    for row in rows:
        market_id = str(row.get("market_id", "")).strip()
        if market_id not in enabled_market_ids:
            continue
        state = str(row.get("state", "")).strip().lower()
        if state not in tracked_states:
            continue
        offer_id = str(row.get("offer_id", "")).strip()
        if not offer_id:
            continue
        market_offer_ids = offer_ids_by_market.setdefault(market_id, [])
        if offer_id in market_offer_ids:
            continue
        if len(market_offer_ids) >= max(1, int(per_market_limit)):
            continue
        market_offer_ids.append(offer_id)

    checked_offer_count = 0
    requeue_market_ids: set[str] = set()
    hits: list[dict[str, str]] = []
    for market_id, offer_ids in offer_ids_by_market.items():
        for offer_id in offer_ids:
            if checked_offer_count >= max(1, int(max_offer_checks)):
                return {
                    "checked_offer_count": checked_offer_count,
                    "requeue_market_ids": sorted(requeue_market_ids),
                    "hits": hits,
                    "truncated": True,
                }
            checked_offer_count += 1
            try:
                payload = dexie.get_offer(offer_id, timeout=5)
                offer = payload.get("offer") if isinstance(payload, dict) else None
                if not isinstance(offer, dict):
                    continue
                status = int(offer.get("status", -1))
                if status in {4, 6}:
                    reason = "tx_confirmed" if status == 4 else "offer_expired"
                    requeue_market_ids.add(market_id)
                    hits.append(
                        {
                            "market_id": market_id,
                            "offer_id": offer_id,
                            "reason": reason,
                        }
                    )
            except Exception as exc:  # pragma: no cover - network dependent
                if _is_dexie_offer_missing_error(exc):
                    requeue_market_ids.add(market_id)
                    hits.append(
                        {
                            "market_id": market_id,
                            "offer_id": offer_id,
                            "reason": "offer_missing_404",
                        }
                    )
                continue

    return {
        "checked_offer_count": checked_offer_count,
        "requeue_market_ids": sorted(requeue_market_ids),
        "hits": hits,
        "truncated": False,
    }


def _evaluate_and_execute_strategy(
    *,
    market: Any,
    program: Any,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    dexie_size_by_offer_id: dict[str, int],
    result: _MarketCycleResult,
    reservation_coordinator: AssetReservationCoordinator | None = None,
) -> tuple[dict[str, dict[int, int]], dict[int, int]]:
    """Evaluate market strategy, inject reseed if needed, and execute offer actions."""
    market_mode = str(getattr(market, "mode", "")).strip().lower()
    strategy_config = _strategy_config_from_market(market)
    tracked_sizes = {
        int(entry.size_base_units)
        for side_entries in (getattr(market, "ladders", {}) or {}).values()
        for entry in side_entries
        if int(getattr(entry, "size_base_units", 0)) > 0
    }
    if not tracked_sizes:
        tracked_sizes = set(_strategy_target_counts_by_size(strategy_config).keys())
    if market_mode == "two_sided":
        offer_counts_by_side, offer_state_counts, active_unmapped_offer_ids = (
            _active_offer_counts_by_size_and_side(
                store=store,
                market_id=market.market_id,
                clock=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                tracked_sizes=tracked_sizes,
            )
        )
        active_offer_counts_by_size = {
            size: int(offer_counts_by_side["buy"].get(size, 0))
            + int(offer_counts_by_side["sell"].get(size, 0))
            for size in sorted(tracked_sizes)
        }
    else:
        active_offer_counts_by_size, offer_state_counts, active_unmapped_offer_ids = (
            _active_offer_counts_by_size(
                store=store,
                market_id=market.market_id,
                clock=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                tracked_sizes=tracked_sizes,
            )
        )
        offer_counts_by_side = {
            "buy": {size: 0 for size in sorted(tracked_sizes)},
            "sell": dict(active_offer_counts_by_size),
        }
    _log_market_decision(
        market.market_id,
        "strategy_state_source",
        source="dexie_offer_coverage",
        active_offer_counts_by_size=active_offer_counts_by_size,
        active_offer_counts_by_side=offer_counts_by_side,
        state_counts=offer_state_counts,
        active_unmapped_offer_ids=active_unmapped_offer_ids,
    )
    if market_mode == "two_sided":
        strategy_actions = _evaluate_two_sided_market_actions(
            market=market,
            counts_by_side=offer_counts_by_side,
            xch_price_usd=xch_price_usd,
            now=now,
        )
    else:
        strategy_actions = evaluate_market(
            state=_strategy_state_from_bucket_counts(
                active_offer_counts_by_size, xch_price_usd=xch_price_usd
            ),
            config=strategy_config,
            clock=now,
        )
    strategy_actions = [action for action in strategy_actions if int(action.repeat) > 0]
    _log_market_decision(
        market.market_id,
        "strategy_evaluated",
        pair=strategy_config.pair,
        mode=market_mode or "sell_only",
        offer_counts=active_offer_counts_by_size,
        xch_price_usd=xch_price_usd,
        action_count=len(strategy_actions),
        cadence_limited_sizes=[],
    )
    if market_mode != "two_sided":
        strategy_actions = _inject_reseed_action_if_no_active_offers(
            strategy_actions=strategy_actions,
            strategy_config=strategy_config,
            market=market,
            store=store,
            xch_price_usd=xch_price_usd,
            clock=now,
            dexie_size_by_offer_id=dexie_size_by_offer_id,
        )
    _log_market_decision(
        market.market_id,
        "strategy_after_reseed",
        action_count=len(strategy_actions),
        reseed_injected=any(
            str(action.reason) == "no_active_offer_reseed" for action in strategy_actions
        ),
    )
    store.add_audit_event(
        "strategy_actions_planned",
        {
            "market_id": market.market_id,
            "xch_price_usd": xch_price_usd,
            "actions": [
                {
                    "size": action.size,
                    "repeat": action.repeat,
                    "pair": action.pair,
                    "expiry_unit": action.expiry_unit,
                    "expiry_value": action.expiry_value,
                    "cancel_after_create": action.cancel_after_create,
                    "reason": action.reason,
                    "target_spread_bps": action.target_spread_bps,
                    "side": _normalize_offer_side(getattr(action, "side", "sell")),
                }
                for action in strategy_actions
            ],
        },
        market_id=market.market_id,
    )
    offer_execution = _execute_strategy_actions(
        market=market,
        strategy_actions=strategy_actions,
        runtime_dry_run=program.runtime_dry_run,
        xch_price_usd=xch_price_usd,
        dexie=dexie,
        splash=splash,
        publish_venue=program.offer_publish_venue,
        store=store,
        app_network=program.app_network,
        signer_key_registry=program.signer_key_registry,
        program=program,
        reservation_coordinator=reservation_coordinator,
    )
    result.strategy_planned += int(offer_execution["planned_count"])
    result.strategy_executed += int(offer_execution["executed_count"])
    _log_market_decision(
        market.market_id,
        "strategy_executed",
        planned_count=offer_execution["planned_count"],
        executed_count=offer_execution["executed_count"],
    )
    store.add_audit_event(
        "strategy_offer_execution",
        {
            "market_id": market.market_id,
            "planned_count": offer_execution["planned_count"],
            "executed_count": offer_execution["executed_count"],
            "items": offer_execution["items"],
        },
        market_id=market.market_id,
    )
    health_payload = _managed_offer_market_health_payload(
        market_id=str(market.market_id),
        current_items=list(offer_execution["items"]),
        now=now,
    )
    store.add_audit_event(
        "managed_offer_market_health",
        health_payload,
        market_id=market.market_id,
    )
    return offer_counts_by_side, _executed_sell_offer_counts_by_size(offer_execution)


def _process_single_market(
    *,
    market: Any,
    program: Any,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None = None,
) -> _MarketCycleResult:
    result = _MarketCycleResult()
    _log_market_decision(
        market.market_id,
        "cycle_start",
        mode=str(getattr(market, "mode", "")),
        quote_asset=str(getattr(market, "quote_asset", "")),
    )
    signer_selection = resolve_market_key(
        market,
        allowed_keys,
        signer_key_registry=program.signer_key_registry,
        required_network=program.app_network,
    )
    _log_market_decision(
        market.market_id,
        "signer_selected",
        key_id=signer_selection.key_id,
        network=program.app_network,
    )
    store.add_price_policy_snapshot(
        market.market_id,
        {
            "mode": market.mode,
            "base_asset": market.base_asset,
            "quote_asset": market.quote_asset,
            "quote_asset_type": market.quote_asset_type,
        },
        source="startup",
    )
    persisted = store.get_alert_state(market.market_id)
    state, event = evaluate_low_inventory_alert(
        now=now,
        program=program,
        market=market,
        state=AlertState(
            is_low=persisted.is_low,
            last_alert_at=persisted.last_alert_at,
        ),
    )
    store.upsert_alert_state(
        StoredAlertState(
            market_id=market.market_id,
            is_low=state.is_low,
            last_alert_at=state.last_alert_at,
        )
    )
    if event:
        payload = {
            "event": "low_inventory_alert",
            "market_id": event.market_id,
            "ticker": event.ticker,
            "remaining_amount": event.remaining_amount,
            "receive_address": event.receive_address,
            "reason": event.reason,
        }
        _log_daemon_event(level=logging.INFO, payload=payload)
        store.add_audit_event("low_inventory_alert", payload, market_id=market.market_id)
        send_pushover_alert(program, event)

    _, dexie_size_by_offer_id, _, offers = reconcile_market_cycle_offers(
        market=market,
        network=program.app_network,
        dexie=dexie,
        store=store,
        now=now,
        result=result,
    )

    sell_ladder = market.ladders.get("sell", [])
    ladder_sizes = [e.size_base_units for e in sell_ladder]
    bucket_counts: dict[int, int] | None = None
    wallet_coins: list[int] = []
    coinset_scan_empty = False

    if isinstance(program, ProgramConfig) and signer_offer_path_configured(program):
        try:
            resolved_base_asset_id, _, _ = _resolve_signer_offer_asset_ids_for_reservation(
                program=program,
                market=market,
            )
            wallet_coins = _coinset_spendable_base_unit_coin_amounts(
                program=program,
                market=market,
                resolved_asset_id=resolved_base_asset_id,
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
            )
            coinset_scan_empty = len(wallet_coins) == 0
            if wallet_coins:
                bucket_counts = compute_bucket_counts_from_coins(
                    coin_amounts_base_units=wallet_coins,
                    ladder_sizes=ladder_sizes,
                )
                _log_market_decision(
                    market.market_id,
                    "inventory_scan_wallet",
                    source="coinset",
                    resolved_asset_id=resolved_base_asset_id,
                    coin_count=len(wallet_coins),
                    bucket_counts=bucket_counts,
                )
                store.add_audit_event(
                    "inventory_bucket_scan",
                    {
                        "market_id": market.market_id,
                        "source": "coinset",
                        "resolved_asset_id": resolved_base_asset_id,
                        "bucket_counts": bucket_counts,
                        "coin_count": len(wallet_coins),
                    },
                    market_id=market.market_id,
                )
        except Exception as exc:
            _daemon_logger.warning(
                "coinset_inventory_scan_failed market_id=%s error=%s",
                market.market_id,
                exc,
            )

    if bucket_counts is None or coinset_scan_empty:
        fallback_source = (
            "wallet_adapter_fallback_after_empty_coinset_scan"
            if coinset_scan_empty
            else "wallet_adapter"
        )
        if coinset_scan_empty and str(market.base_asset).strip().lower() not in {
            "xch",
            "1",
            "",
        }:
            wallet_coins = _coinset_cat_spendable_base_unit_coin_amounts(
                canonical_asset_id=str(market.base_asset),
                receive_address=str(market.receive_address),
                network=str(program.app_network),
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
            )
            if wallet_coins:
                fallback_source = "coinset_cat_scan_fallback_after_empty_coinset_scan"
        if not wallet_coins:
            wallet_coins = wallet.list_asset_coins_base_units(
                asset_id=market.base_asset,
                key_id=market.signer_key_id,
                receive_address=market.receive_address,
                network=program.app_network,
            )
        if wallet_coins:
            bucket_counts = compute_bucket_counts_from_coins(
                coin_amounts_base_units=wallet_coins,
                ladder_sizes=ladder_sizes,
            )
            _log_market_decision(
                market.market_id,
                "inventory_scan_wallet",
                source=fallback_source,
                coin_count=len(wallet_coins),
                bucket_counts=bucket_counts,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": fallback_source,
                    "bucket_counts": bucket_counts,
                    "coin_count": len(wallet_coins),
                },
                market_id=market.market_id,
            )
        else:
            bucket_counts = dict(market.inventory.bucket_counts)
            _log_market_decision(
                market.market_id,
                "inventory_scan_config_fallback",
                asset_id=market.base_asset,
                bucket_counts=bucket_counts,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": "config_seed_or_no_asset_scan",
                    "asset_id": market.base_asset,
                    "bucket_counts": bucket_counts,
                },
                market_id=market.market_id,
            )
    offer_counts_by_side: dict[str, dict[int, int]] = {"buy": {}, "sell": {}}
    newly_executed_sell_offer_counts_by_size: dict[int, int] = {}
    try:
        offer_counts_by_side, newly_executed_sell_offer_counts_by_size = (
            _evaluate_and_execute_strategy(
                market=market,
                program=program,
                dexie=dexie,
                splash=splash,
                store=store,
                xch_price_usd=xch_price_usd,
                now=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                result=result,
                reservation_coordinator=reservation_coordinator,
            )
        )
    except Exception as exc:
        result.cycle_errors += 1
        _log_market_decision(
            market.market_id,
            "strategy_failed",
            error=str(exc),
        )
        store.add_audit_event(
            "strategy_execution_error",
            {"market_id": market.market_id, "error": str(exc)},
            market_id=market.market_id,
        )
    # Cancel uses the Dexie offer list fetched at cycle start (before strategy).
    # Intentionally stale relative to same-cycle posts so cancel policy cannot
    # target offers strategy just created (avoids cancel-then-reseed races).
    cancel_policy = _execute_cancel_policy_for_market(
        market=market,
        offers=offers,
        runtime_dry_run=program.runtime_dry_run,
        current_xch_price_usd=xch_price_usd,
        previous_xch_price_usd=previous_xch_price_usd,
        dexie=dexie,
        store=store,
    )
    if bool(cancel_policy.get("triggered", False)):
        result.cancel_triggered = True
    result.cancel_planned += int(cancel_policy.get("planned_count", 0))
    result.cancel_executed += int(cancel_policy.get("executed_count", 0))
    _log_market_decision(
        market.market_id,
        "cancel_policy_evaluated",
        eligible=cancel_policy["eligible"],
        triggered=cancel_policy["triggered"],
        reason=cancel_policy["reason"],
        move_bps=cancel_policy["move_bps"],
        threshold_bps=cancel_policy["threshold_bps"],
        planned_count=cancel_policy["planned_count"],
        executed_count=cancel_policy["executed_count"],
    )
    store.add_audit_event(
        "offer_cancel_policy",
        {
            "market_id": market.market_id,
            "eligible": cancel_policy["eligible"],
            "triggered": cancel_policy["triggered"],
            "reason": cancel_policy["reason"],
            "move_bps": cancel_policy["move_bps"],
            "threshold_bps": cancel_policy["threshold_bps"],
            "planned_count": cancel_policy["planned_count"],
            "executed_count": cancel_policy["executed_count"],
            "items": cancel_policy["items"],
        },
        market_id=market.market_id,
    )
    try:
        _plan_and_execute_coin_ops(
            market=market,
            program=program,
            wallet=wallet,
            store=store,
            sell_ladder=sell_ladder,
            wallet_bucket_counts=bucket_counts,
            active_sell_offer_counts_by_size=offer_counts_by_side.get("sell", {}),
            newly_executed_sell_offer_counts_by_size=newly_executed_sell_offer_counts_by_size,
            signer_selection=signer_selection,
            state_dir=state_dir,
        )
    except Exception as exc:
        result.cycle_errors += 1
        _log_market_decision(
            market.market_id,
            "coin_ops_failed",
            error=str(exc),
        )
        store.add_audit_event(
            "coin_ops_execution_error",
            {"market_id": market.market_id, "error": str(exc)},
            market_id=market.market_id,
        )
    _log_market_decision(
        market.market_id,
        "cycle_complete",
        cycle_errors=result.cycle_errors,
        strategy_planned=result.strategy_planned,
        strategy_executed=result.strategy_executed,
        cancel_triggered=result.cancel_triggered,
        cancel_planned=result.cancel_planned,
        cancel_executed=result.cancel_executed,
    )
    return result


def _process_single_market_with_store(
    *,
    market: Any,
    program: Any,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    db_path: Path,
    xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None = None,
) -> _MarketCycleResult:
    """Run one market cycle with a thread-local SQLite connection."""
    store = SqliteStore(db_path)
    try:
        return _process_single_market(
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
    finally:
        store.close()


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
        if (
            market_dispatch_state is not None
            and configured_market_slot_count > 0
            and len(enabled_markets) > configured_market_slot_count
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
        deduped_requeue_market_ids = sorted({mid for mid in immediate_requeue_market_ids if mid})
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
