"""Per-market daemon cycle phases (reconcile, inventory, strategy, cancel, coin ops)."""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.models import ProgramConfig, signer_offer_path_configured
from greenfloor.core.cycle import (
    MARKET_CYCLE_PHASES,
    aggregate_two_sided_offer_counts,
    is_two_sided_market_mode,
    needs_inventory_fallback,
    one_sided_offer_counts_by_side,
    resolve_inventory_scan_source,
    resolve_tracked_sizes,
    should_try_cat_inventory_fallback,
)
from greenfloor.core.inventory import compute_bucket_counts_from_coins
from greenfloor.core.notifications import AlertState, evaluate_low_inventory_alert
from greenfloor.core.strategy import evaluate_market
from greenfloor.daemon.cancel_policy import _execute_cancel_policy_for_market
from greenfloor.daemon.coin_ops_cycle import (
    _executed_sell_offer_counts_by_size,
    _plan_and_execute_coin_ops,
)
from greenfloor.daemon.cooldowns import _managed_offer_market_health_payload
from greenfloor.daemon.inventory_scan import (
    _coinset_cat_spendable_base_unit_coin_amounts,
    _coinset_spendable_base_unit_coin_amounts,
)
from greenfloor.daemon.market_helpers import (
    _base_unit_mojo_multiplier_for_market,
    _normalize_offer_side,
)
from greenfloor.daemon.market_logging import _daemon_logger, _log_market_decision
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
    _strategy_target_counts_by_size,
)
from greenfloor.keys.router import resolve_market_key
from greenfloor.notify.pushover import send_pushover_alert
from greenfloor.storage.sqlite import SqliteStore, StoredAlertState

__all__ = [
    "MarketCycleResult",
    "evaluate_and_execute_strategy",
    "process_single_market",
    "process_single_market_with_store",
]


@dataclass(slots=True)
class MarketCycleResult:
    cycle_errors: int = 0
    strategy_planned: int = 0
    strategy_executed: int = 0
    cancel_triggered: bool = False
    cancel_planned: int = 0
    cancel_executed: int = 0
    immediate_requeue_requested: bool = False
    immediate_requeue_signals: list[str] = field(default_factory=list)

    def record_phase_error(self) -> None:
        self.cycle_errors += 1

    def merge_strategy_execution(self, *, planned: int, executed: int) -> None:
        self.strategy_planned += max(0, int(planned))
        self.strategy_executed += max(0, int(executed))

    def merge_cancel_policy(self, *, triggered: bool, planned: int, executed: int) -> None:
        if triggered:
            self.cancel_triggered = True
        self.cancel_planned += max(0, int(planned))
        self.cancel_executed += max(0, int(executed))


def _log_daemon_event(*, level: int, payload: dict[str, Any]) -> None:
    _daemon_logger.log(level, "daemon_event %s", json.dumps(payload, sort_keys=True))


def _resolve_tracked_sizes_for_market(*, market: Any, strategy_config: Any) -> list[int]:
    ladder_sizes = [
        int(getattr(entry, "size_base_units", 0))
        for side_entries in (getattr(market, "ladders", {}) or {}).values()
        for entry in side_entries
    ]
    return resolve_tracked_sizes(
        ladder_sizes=ladder_sizes,
        strategy_default_sizes=list(_strategy_target_counts_by_size(strategy_config).keys()),
    )


def evaluate_and_execute_strategy(
    *,
    market: Any,
    program: Any,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    dexie_size_by_offer_id: dict[str, int],
    result: MarketCycleResult,
    reservation_coordinator: AssetReservationCoordinator | None = None,
) -> tuple[dict[str, dict[int, int]], dict[int, int]]:
    """Evaluate market strategy, inject reseed if needed, and execute offer actions."""
    market_mode = str(getattr(market, "mode", "")).strip().lower()
    strategy_config = _strategy_config_from_market(market)
    tracked_sizes_list = _resolve_tracked_sizes_for_market(
        market=market,
        strategy_config=strategy_config,
    )
    tracked_sizes = set(tracked_sizes_list)
    two_sided = is_two_sided_market_mode(market_mode)
    if two_sided:
        offer_counts_by_side, offer_state_counts, active_unmapped_offer_ids = (
            _active_offer_counts_by_size_and_side(
                store=store,
                market_id=market.market_id,
                clock=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                tracked_sizes=tracked_sizes,
            )
        )
        active_offer_counts_by_size = aggregate_two_sided_offer_counts(
            buy_counts=offer_counts_by_side["buy"],
            sell_counts=offer_counts_by_side["sell"],
            tracked_sizes=tracked_sizes_list,
        )
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
        buy_side, sell_side = one_sided_offer_counts_by_side(
            sell_counts=active_offer_counts_by_size,
            tracked_sizes=tracked_sizes_list,
        )
        offer_counts_by_side = {"buy": buy_side, "sell": sell_side}
    _log_market_decision(
        market.market_id,
        "strategy_state_source",
        source="dexie_offer_coverage",
        active_offer_counts_by_size=active_offer_counts_by_size,
        active_offer_counts_by_side=offer_counts_by_side,
        state_counts=offer_state_counts,
        active_unmapped_offer_ids=active_unmapped_offer_ids,
    )
    if two_sided:
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
    if not two_sided:
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
    result.merge_strategy_execution(
        planned=int(offer_execution["planned_count"]),
        executed=int(offer_execution["executed_count"]),
    )
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


def _run_market_cycle_setup(
    *,
    market: Any,
    program: Any,
    allowed_keys: set[str] | None,
    store: SqliteStore,
    now: datetime,
) -> Any:
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
    return signer_selection


def _run_market_cycle_inventory(
    *,
    market: Any,
    program: Any,
    wallet: WalletAdapter,
    store: SqliteStore,
    sell_ladder: list[Any],
) -> dict[int, int] | None:
    ladder_sizes = [e.size_base_units for e in sell_ladder]
    bucket_counts: dict[int, int] | None = None
    wallet_coins: list[int] = []
    coinset_scan_empty = False
    coinset_scan_found_coins = False
    cat_scan_found_coins = False
    wallet_scan_found_coins = False

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
                coinset_scan_found_coins = True
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

    if needs_inventory_fallback(
        bucket_counts_available=bucket_counts is not None,
        coinset_scan_empty=coinset_scan_empty,
    ):
        wallet_coins = []
        if should_try_cat_inventory_fallback(
            coinset_scan_empty=coinset_scan_empty,
            base_asset=str(market.base_asset),
        ):
            wallet_coins = _coinset_cat_spendable_base_unit_coin_amounts(
                canonical_asset_id=str(market.base_asset),
                receive_address=str(market.receive_address),
                network=str(program.app_network),
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
            )
            cat_scan_found_coins = len(wallet_coins) > 0
        if not wallet_coins:
            wallet_coins = wallet.list_asset_coins_base_units(
                asset_id=market.base_asset,
                key_id=market.signer_key_id,
                receive_address=market.receive_address,
                network=program.app_network,
            )
            wallet_scan_found_coins = len(wallet_coins) > 0
        if wallet_coins:
            bucket_counts = compute_bucket_counts_from_coins(
                coin_amounts_base_units=wallet_coins,
                ladder_sizes=ladder_sizes,
            )
            fallback_source = resolve_inventory_scan_source(
                coinset_scan_found_coins=coinset_scan_found_coins,
                coinset_scan_empty=coinset_scan_empty,
                cat_scan_found_coins=cat_scan_found_coins,
                wallet_scan_found_coins=wallet_scan_found_coins,
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
            fallback_source = resolve_inventory_scan_source(
                coinset_scan_found_coins=coinset_scan_found_coins,
                coinset_scan_empty=coinset_scan_empty,
                cat_scan_found_coins=cat_scan_found_coins,
                wallet_scan_found_coins=wallet_scan_found_coins,
            )
            _log_market_decision(
                market.market_id,
                "inventory_scan_config_fallback",
                asset_id=market.base_asset,
                bucket_counts=bucket_counts,
                source=fallback_source,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": fallback_source,
                    "asset_id": market.base_asset,
                    "bucket_counts": bucket_counts,
                },
                market_id=market.market_id,
            )
    return bucket_counts


@dataclass(slots=True)
class _MarketCycleRun:
    market: Any
    program: Any
    allowed_keys: set[str] | None
    dexie: DexieAdapter
    splash: SplashAdapter
    wallet: WalletAdapter
    store: SqliteStore
    xch_price_usd: float | None
    previous_xch_price_usd: float | None
    now: datetime
    state_dir: Path
    reservation_coordinator: AssetReservationCoordinator | None
    result: MarketCycleResult
    signer_selection: Any = None
    dexie_size_by_offer_id: dict[str, int] = field(default_factory=dict)
    offers: list[Any] = field(default_factory=list)
    sell_ladder: list[Any] = field(default_factory=list)
    bucket_counts: dict[int, int] | None = None
    offer_counts_by_side: dict[str, dict[int, int]] = field(
        default_factory=lambda: {"buy": {}, "sell": {}}
    )
    newly_executed_sell_offer_counts_by_size: dict[int, int] = field(default_factory=dict)


def _run_market_cycle_reconcile_phase(run: _MarketCycleRun) -> None:
    _, run.dexie_size_by_offer_id, _, run.offers = reconcile_market_cycle_offers(
        market=run.market,
        network=run.program.app_network,
        dexie=run.dexie,
        store=run.store,
        now=run.now,
        result=run.result,
    )


def _run_market_cycle_inventory_phase(run: _MarketCycleRun) -> None:
    run.sell_ladder = run.market.ladders.get("sell", [])
    run.bucket_counts = _run_market_cycle_inventory(
        market=run.market,
        program=run.program,
        wallet=run.wallet,
        store=run.store,
        sell_ladder=run.sell_ladder,
    )


def _run_market_cycle_strategy_phase(run: _MarketCycleRun) -> None:
    try:
        run.offer_counts_by_side, run.newly_executed_sell_offer_counts_by_size = (
            evaluate_and_execute_strategy(
                market=run.market,
                program=run.program,
                dexie=run.dexie,
                splash=run.splash,
                store=run.store,
                xch_price_usd=run.xch_price_usd,
                now=run.now,
                dexie_size_by_offer_id=run.dexie_size_by_offer_id,
                result=run.result,
                reservation_coordinator=run.reservation_coordinator,
            )
        )
    except Exception as exc:
        run.result.record_phase_error()
        _log_market_decision(
            run.market.market_id,
            "strategy_failed",
            error=str(exc),
        )
        run.store.add_audit_event(
            "strategy_execution_error",
            {"market_id": run.market.market_id, "error": str(exc)},
            market_id=run.market.market_id,
        )


def _run_market_cycle_cancel_phase(run: _MarketCycleRun) -> None:
    cancel_policy = _execute_cancel_policy_for_market(
        market=run.market,
        offers=run.offers,
        runtime_dry_run=run.program.runtime_dry_run,
        current_xch_price_usd=run.xch_price_usd,
        previous_xch_price_usd=run.previous_xch_price_usd,
        dexie=run.dexie,
        store=run.store,
    )
    run.result.merge_cancel_policy(
        triggered=bool(cancel_policy.get("triggered", False)),
        planned=int(cancel_policy.get("planned_count", 0)),
        executed=int(cancel_policy.get("executed_count", 0)),
    )
    _log_market_decision(
        run.market.market_id,
        "cancel_policy_evaluated",
        eligible=cancel_policy["eligible"],
        triggered=cancel_policy["triggered"],
        reason=cancel_policy["reason"],
        move_bps=cancel_policy["move_bps"],
        threshold_bps=cancel_policy["threshold_bps"],
        planned_count=cancel_policy["planned_count"],
        executed_count=cancel_policy["executed_count"],
    )
    run.store.add_audit_event(
        "offer_cancel_policy",
        {
            "market_id": run.market.market_id,
            "eligible": cancel_policy["eligible"],
            "triggered": cancel_policy["triggered"],
            "reason": cancel_policy["reason"],
            "move_bps": cancel_policy["move_bps"],
            "threshold_bps": cancel_policy["threshold_bps"],
            "planned_count": cancel_policy["planned_count"],
            "executed_count": cancel_policy["executed_count"],
            "items": cancel_policy["items"],
        },
        market_id=run.market.market_id,
    )


def _run_market_cycle_coin_ops_phase(run: _MarketCycleRun) -> None:
    try:
        _plan_and_execute_coin_ops(
            market=run.market,
            program=run.program,
            wallet=run.wallet,
            store=run.store,
            sell_ladder=run.sell_ladder,
            wallet_bucket_counts=run.bucket_counts or dict(run.market.inventory.bucket_counts),
            active_sell_offer_counts_by_size=run.offer_counts_by_side.get("sell", {}),
            newly_executed_sell_offer_counts_by_size=run.newly_executed_sell_offer_counts_by_size,
            signer_selection=run.signer_selection,
            state_dir=run.state_dir,
        )
    except Exception as exc:
        run.result.record_phase_error()
        _log_market_decision(
            run.market.market_id,
            "coin_ops_failed",
            error=str(exc),
        )
        run.store.add_audit_event(
            "coin_ops_execution_error",
            {"market_id": run.market.market_id, "error": str(exc)},
            market_id=run.market.market_id,
        )


_MARKET_CYCLE_PHASE_RUNNERS = {
    "reconcile": _run_market_cycle_reconcile_phase,
    "inventory": _run_market_cycle_inventory_phase,
    "strategy": _run_market_cycle_strategy_phase,
    "cancel": _run_market_cycle_cancel_phase,
    "coin_ops": _run_market_cycle_coin_ops_phase,
}


def process_single_market(
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
) -> MarketCycleResult:
    run = _MarketCycleRun(
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
        result=MarketCycleResult(),
    )
    run.signer_selection = _run_market_cycle_setup(
        market=run.market,
        program=run.program,
        allowed_keys=run.allowed_keys,
        store=run.store,
        now=run.now,
    )
    for phase in MARKET_CYCLE_PHASES:
        _MARKET_CYCLE_PHASE_RUNNERS[phase](run)
    _log_market_decision(
        run.market.market_id,
        "cycle_complete",
        cycle_errors=run.result.cycle_errors,
        strategy_planned=run.result.strategy_planned,
        strategy_executed=run.result.strategy_executed,
        cancel_triggered=run.result.cancel_triggered,
        cancel_planned=run.result.cancel_planned,
        cancel_executed=run.result.cancel_executed,
    )
    return run.result


def process_single_market_with_store(
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
) -> MarketCycleResult:
    """Run one market cycle with a thread-local SQLite connection."""
    store = SqliteStore(db_path)
    try:
        return process_single_market(
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
