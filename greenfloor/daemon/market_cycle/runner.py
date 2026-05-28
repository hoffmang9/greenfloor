"""Per-market daemon cycle orchestration."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.market_cycle.phases import MarketCycleRun, run_market_cycle_phases
from greenfloor.daemon.market_cycle.result import MarketCycleResult
from greenfloor.daemon.market_cycle.setup_phase import run_market_cycle_setup
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.storage.sqlite import SqliteStore


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
    run = MarketCycleRun(
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
    run.signer_selection = run_market_cycle_setup(
        market=run.market,
        program=run.program,
        allowed_keys=run.allowed_keys,
        store=run.store,
        now=run.now,
    )
    run_market_cycle_phases(run)
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
