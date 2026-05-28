"""Parallel and sequential market dispatch within a single daemon cycle."""

from __future__ import annotations

import concurrent.futures
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.models import ProgramConfig
from greenfloor.daemon.market_cycle import (
    process_single_market,
    process_single_market_with_store,
)
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.storage.sqlite import SqliteStore


@dataclass(frozen=True, slots=True)
class MarketCycleDispatchResult:
    markets_processed: int
    cycle_error_count: int
    strategy_planned_total: int
    strategy_executed_total: int
    cancel_triggered_count: int
    cancel_planned_total: int
    cancel_executed_total: int
    immediate_requeue_market_ids: list[str]


def _record_market_worker_error(
    *,
    store: SqliteStore,
    market_id: str,
    exc: Exception,
    source: str,
) -> None:
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
            "source": source,
        },
    )


def dispatch_selected_markets(
    *,
    program: ProgramConfig,
    selected_markets: list[Any],
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    store: SqliteStore,
    db_path: Path,
    xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    now: Any,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None,
    parallel_markets_enabled: bool,
) -> MarketCycleDispatchResult:
    markets_processed = 0
    cycle_error_count = 0
    strategy_planned_total = 0
    strategy_executed_total = 0
    cancel_triggered_count = 0
    cancel_planned_total = 0
    cancel_executed_total = 0
    immediate_requeue_market_ids: list[str] = []

    if parallel_markets_enabled and len(selected_markets) > 1:
        max_workers = max(1, len(selected_markets))
        with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
            future_to_market = {
                pool.submit(
                    process_single_market_with_store,
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
                    _record_market_worker_error(
                        store=store,
                        market_id=market_id,
                        exc=exc,
                        source="parallel_market_worker",
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
        for market in selected_markets:
            market_id = str(getattr(market, "market_id", "")).strip()
            try:
                mr = process_single_market(
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
                _record_market_worker_error(
                    store=store,
                    market_id=market_id,
                    exc=exc,
                    source="sequential_market_worker",
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

    return MarketCycleDispatchResult(
        markets_processed=markets_processed,
        cycle_error_count=cycle_error_count,
        strategy_planned_total=strategy_planned_total,
        strategy_executed_total=strategy_executed_total,
        cancel_triggered_count=cancel_triggered_count,
        cancel_planned_total=cancel_planned_total,
        cancel_executed_total=cancel_executed_total,
        immediate_requeue_market_ids=immediate_requeue_market_ids,
    )
