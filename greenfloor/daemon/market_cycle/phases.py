"""Market cycle phase runners (reconcile through coin ops)."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.cycle import MARKET_CYCLE_PHASES
from greenfloor.daemon.cancel_policy import _execute_cancel_policy_for_market
from greenfloor.daemon.coin_ops_cycle import _plan_and_execute_coin_ops
from greenfloor.daemon.market_cycle.inventory_phase import run_market_cycle_inventory
from greenfloor.daemon.market_cycle.result import MarketCycleResult
from greenfloor.daemon.market_cycle.strategy_phase import evaluate_and_execute_strategy
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.offer_reconcile_cycle import reconcile_market_cycle_offers
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.storage.sqlite import SqliteStore


@dataclass(slots=True)
class MarketCycleRun:
    market: MarketConfig
    program: ProgramConfig
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


def run_market_cycle_reconcile_phase(run: MarketCycleRun) -> None:
    _, run.dexie_size_by_offer_id, _, run.offers = reconcile_market_cycle_offers(
        market=run.market,
        network=run.program.app_network,
        dexie=run.dexie,
        store=run.store,
        now=run.now,
        result=run.result,
    )


def run_market_cycle_inventory_phase(run: MarketCycleRun) -> None:
    run.sell_ladder = run.market.ladders.get("sell", [])
    run.bucket_counts = run_market_cycle_inventory(
        market=run.market,
        program=run.program,
        wallet=run.wallet,
        store=run.store,
        sell_ladder=run.sell_ladder,
    )


def run_market_cycle_strategy_phase(run: MarketCycleRun) -> None:
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


def run_market_cycle_cancel_phase(run: MarketCycleRun) -> None:
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


def run_market_cycle_coin_ops_phase(run: MarketCycleRun) -> None:
    try:
        _plan_and_execute_coin_ops(
            market=run.market,
            program=run.program,
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


MARKET_CYCLE_PHASE_RUNNERS = {
    "reconcile": run_market_cycle_reconcile_phase,
    "inventory": run_market_cycle_inventory_phase,
    "strategy": run_market_cycle_strategy_phase,
    "cancel": run_market_cycle_cancel_phase,
    "coin_ops": run_market_cycle_coin_ops_phase,
}


def run_market_cycle_phases(run: MarketCycleRun) -> None:
    for phase in MARKET_CYCLE_PHASES:
        MARKET_CYCLE_PHASE_RUNNERS[phase](run)
