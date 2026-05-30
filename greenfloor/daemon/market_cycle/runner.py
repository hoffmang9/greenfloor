"""Per-market daemon cycle orchestration."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.models import MarketConfig, MarketLadderEntry, ProgramConfig
from greenfloor.daemon.market_cycle.phases import (
    MarketCycleRun,
    run_market_cycle_coin_ops_phase,
    run_market_cycle_inventory_phase,
    run_market_cycle_phases,
    run_market_cycle_strategy_phase,
)
from greenfloor.daemon.market_cycle.result import MarketCycleResult
from greenfloor.daemon.market_cycle.setup_phase import run_market_cycle_setup
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.runtime.offer_watchlist import update_market_coin_watchlist_from_dexie
from greenfloor.storage.sqlite import SqliteStore


def _build_market_cycle_run(
    *,
    market: MarketConfig,
    program: ProgramConfig,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None,
    reconcile_context: dict,
) -> MarketCycleRun:
    offers = reconcile_context.get("offers", [])
    if not isinstance(offers, list):
        offers = []
    dexie_size_raw = reconcile_context.get("dexie_size_by_offer_id", {})
    dexie_size_by_offer_id = (
        {str(key): int(value) for key, value in dexie_size_raw.items()}
        if isinstance(dexie_size_raw, dict)
        else {}
    )
    return MarketCycleRun(
        market=market,
        program=program,
        allowed_keys=allowed_keys,
        dexie=dexie,
        splash=splash,
        wallet=wallet,
        store=store,
        xch_price_usd=xch_price_usd,
        previous_xch_price_usd=None,
        now=now,
        state_dir=state_dir,
        reservation_coordinator=reservation_coordinator,
        result=MarketCycleResult(),
        offers=offers,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
    )


def process_single_market_python_phases(
    *,
    market: MarketConfig,
    program: ProgramConfig,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None,
    reconcile_context: dict,
) -> dict:
    run = _build_market_cycle_run(
        market=market,
        program=program,
        allowed_keys=allowed_keys,
        dexie=dexie,
        splash=splash,
        wallet=wallet,
        store=store,
        xch_price_usd=xch_price_usd,
        now=now,
        state_dir=state_dir,
        reservation_coordinator=reservation_coordinator,
        reconcile_context=reconcile_context,
    )
    run.signer_selection = run_market_cycle_setup(
        market=run.market,
        program=run.program,
        allowed_keys=run.allowed_keys,
        store=run.store,
        now=run.now,
    )
    if reconcile_context.get("dexie_fetch_error") is None:
        update_market_coin_watchlist_from_dexie(
            market=run.market,
            offers=run.offers,
            store=run.store,
            clock=run.now,
        )
    run_market_cycle_inventory_phase(run)
    run_market_cycle_strategy_phase(run)
    run_market_cycle_coin_ops_phase(run)
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
    return {
        "cycle_error_count": int(run.result.cycle_errors),
        "strategy_planned_total": int(run.result.strategy_planned),
        "strategy_executed_total": int(run.result.strategy_executed),
    }


def process_single_market_io_phases(
    *,
    market: MarketConfig,
    program: ProgramConfig,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None,
    reconcile_context: dict,
) -> dict:
    run = _build_market_cycle_run(
        market=market,
        program=program,
        allowed_keys=allowed_keys,
        dexie=dexie,
        splash=splash,
        wallet=wallet,
        store=store,
        xch_price_usd=xch_price_usd,
        now=now,
        state_dir=state_dir,
        reservation_coordinator=reservation_coordinator,
        reconcile_context=reconcile_context,
    )
    run.signer_selection = run_market_cycle_setup(
        market=run.market,
        program=run.program,
        allowed_keys=run.allowed_keys,
        store=run.store,
        now=run.now,
    )
    if reconcile_context.get("dexie_fetch_error") is None:
        update_market_coin_watchlist_from_dexie(
            market=run.market,
            offers=run.offers,
            store=run.store,
            clock=run.now,
        )
    run_market_cycle_inventory_phase(run)
    run_market_cycle_strategy_phase(run)
    return {
        "cycle_error_count": int(run.result.cycle_errors),
        "strategy_planned_total": int(run.result.strategy_planned),
        "strategy_executed_total": int(run.result.strategy_executed),
        "coin_ops_payload": {},
        "sell_ladder": [
            {
                "size_base_units": int(entry.size_base_units),
                "target_count": int(entry.target_count),
                "split_buffer_count": int(entry.split_buffer_count),
            }
            for entry in run.sell_ladder
        ],
        "bucket_counts": dict(run.bucket_counts or {}),
        "offer_counts_by_side": {
            side: {int(size): int(count) for size, count in counts.items()}
            for side, counts in run.offer_counts_by_side.items()
        },
        "newly_executed_sell_offer_counts_by_size": {
            int(size): int(count)
            for size, count in run.newly_executed_sell_offer_counts_by_size.items()
        },
    }


def process_single_market_coin_ops_phase(
    *,
    market: MarketConfig,
    program: ProgramConfig,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None,
    io_context: dict,
) -> None:
    del splash, xch_price_usd, reservation_coordinator
    run = _build_market_cycle_run(
        market=market,
        program=program,
        allowed_keys=allowed_keys,
        dexie=dexie,
        splash=SplashAdapter(program.splash_api_base),
        wallet=wallet,
        store=store,
        xch_price_usd=None,
        now=now,
        state_dir=state_dir,
        reservation_coordinator=None,
        reconcile_context={"offers": [], "dexie_size_by_offer_id": {}},
    )
    from greenfloor.keys.router import resolve_market_key

    run.signer_selection = resolve_market_key(
        market,
        allowed_keys,
        signer_key_registry=program.signer_key_registry,
        required_network=program.app_network,
    )
    ladder_raw = io_context.get("sell_ladder", [])
    if isinstance(ladder_raw, list):
        run.sell_ladder = [
            MarketLadderEntry(
                size_base_units=int(entry.get("size_base_units", 0)),
                target_count=int(entry.get("target_count", 0)),
                split_buffer_count=int(entry.get("split_buffer_count", 0)),
                combine_when_excess_factor=float(entry.get("combine_when_excess_factor", 2.0)),
            )
            for entry in ladder_raw
            if isinstance(entry, dict)
        ]
    bucket_raw = io_context.get("bucket_counts")
    run.bucket_counts = (
        {int(size): int(count) for size, count in bucket_raw.items()}
        if isinstance(bucket_raw, dict)
        else None
    )
    offer_counts_raw = io_context.get("offer_counts_by_side", {})
    if isinstance(offer_counts_raw, dict):
        run.offer_counts_by_side = {
            str(side): {int(size): int(count) for size, count in counts.items()}
            for side, counts in offer_counts_raw.items()
            if isinstance(counts, dict)
        }
    newly_executed_raw = io_context.get("newly_executed_sell_offer_counts_by_size", {})
    if isinstance(newly_executed_raw, dict):
        run.newly_executed_sell_offer_counts_by_size = {
            int(size): int(count) for size, count in newly_executed_raw.items()
        }
    run_market_cycle_coin_ops_phase(run)


def process_single_market(
    *,
    market: MarketConfig,
    program: ProgramConfig,
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
    market: MarketConfig,
    program: ProgramConfig,
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
