"""Strategy evaluation and offer execution phase for a market cycle."""

from __future__ import annotations

from datetime import datetime

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.daemon.coin_ops_cycle import _executed_sell_offer_counts_by_size
from greenfloor.daemon.market_cycle.result import MarketCycleResult
from greenfloor.daemon.market_cycle.strategy_eval_phase import (
    evaluate_strategy_for_market,
    resolve_tracked_sizes_for_market,
)
from greenfloor.daemon.market_cycle.strategy_exec_phase import execute_strategy_for_market
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.storage.sqlite import SqliteStore

__all__ = [
    "evaluate_and_execute_strategy",
    "evaluate_strategy_for_market",
    "execute_strategy_for_market",
    "resolve_tracked_sizes_for_market",
]


def evaluate_and_execute_strategy(
    *,
    market: MarketConfig,
    program: ProgramConfig,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    dexie_size_by_offer_id: dict[str, int],
    result: MarketCycleResult,
    reservation_coordinator: AssetReservationCoordinator | None = None,
) -> tuple[dict[str, dict[int, int]], dict[int, int]]:
    strategy_actions, offer_counts_by_side, _active_counts = evaluate_strategy_for_market(
        market=market,
        store=store,
        xch_price_usd=xch_price_usd,
        now=now,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
    )
    offer_execution = execute_strategy_for_market(
        market=market,
        program=program,
        strategy_actions=strategy_actions,
        dexie=dexie,
        splash=splash,
        store=store,
        xch_price_usd=xch_price_usd,
        now=now,
        result=result,
        reservation_coordinator=reservation_coordinator,
    )
    return offer_counts_by_side, _executed_sell_offer_counts_by_size(offer_execution)
