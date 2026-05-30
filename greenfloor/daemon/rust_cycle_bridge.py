"""Python IO bridge invoked from the Rust daemon cycle orchestrator."""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.price import PriceAdapter, XchPriceProvider
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.io import load_markets_config_with_optional_overlay, load_program_config
from greenfloor.config.models import signer_offer_path_configured
from greenfloor.core.notifications import utcnow
from greenfloor.daemon.inventory_scan import (
    _build_coinset_adapter,
    _run_coinset_signal_capture_once,
)
from greenfloor.daemon.market_cycle.runner import process_single_market_python_phases
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.runtime.daemon_config_paths import (
    DaemonConfigPaths,
    set_daemon_config_paths,
)
from greenfloor.storage.sqlite import SqliteStore


def _load_market_context(
    *,
    program_path: str,
    markets_path: str,
    testnet_markets_path: str | None,
    market_id: str,
    allowed_key_ids: list[str],
    db_path: str,
) -> tuple[
    Any,
    Any,
    DexieAdapter,
    SplashAdapter,
    WalletAdapter,
    SqliteStore,
    AssetReservationCoordinator | None,
    set[str] | None,
]:
    program_path_obj = Path(program_path)
    markets_path_obj = Path(markets_path)
    testnet_path = Path(testnet_markets_path) if testnet_markets_path else None
    set_daemon_config_paths(
        DaemonConfigPaths(
            program_path=program_path_obj,
            markets_path=markets_path_obj,
            testnet_markets_path=testnet_path,
        )
    )
    program = load_program_config(program_path_obj)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path_obj,
        overlay_path=testnet_path,
    )
    market = next(
        (
            row
            for row in markets.markets
            if row.enabled and str(row.market_id).strip() == str(market_id).strip()
        ),
        None,
    )
    if market is None:
        raise ValueError(f"enabled market not found: {market_id}")
    allowed_keys = {key.strip() for key in allowed_key_ids if key.strip()} or None
    db_path_obj = Path(db_path)
    store = SqliteStore(db_path_obj)
    reservation_coordinator: AssetReservationCoordinator | None = None
    if bool(program.runtime_offer_parallelism_enabled) and signer_offer_path_configured(program):
        reservation_coordinator = AssetReservationCoordinator(
            db_path=db_path_obj,
            lease_seconds=int(program.runtime_reservation_ttl_seconds),
        )
        expired_count = reservation_coordinator.expire_stale()
        if expired_count > 0:
            store.add_audit_event("reservation_expired", {"count": int(expired_count)})
    dexie = DexieAdapter(program.dexie_api_base)
    splash = SplashAdapter(program.splash_api_base)
    wallet = WalletAdapter()
    return program, market, dexie, splash, wallet, store, reservation_coordinator, allowed_keys


def run_market_cycle_python_phases(
    *,
    program_path: str,
    markets_path: str,
    testnet_markets_path: str | None = None,
    market_id: str,
    allowed_key_ids: list[str],
    db_path: str,
    state_dir: str,
    xch_price_usd: float | None,
    reconcile_context: dict[str, Any],
) -> dict[str, Any]:
    program, market, dexie, splash, wallet, store, reservation_coordinator, allowed_keys = (
        _load_market_context(
            program_path=program_path,
            markets_path=markets_path,
            testnet_markets_path=testnet_markets_path,
            market_id=market_id,
            allowed_key_ids=allowed_key_ids,
            db_path=db_path,
        )
    )
    try:
        return process_single_market_python_phases(
            market=market,
            program=program,
            allowed_keys=allowed_keys,
            dexie=dexie,
            splash=splash,
            wallet=wallet,
            store=store,
            xch_price_usd=xch_price_usd,
            now=utcnow(),
            state_dir=Path(state_dir),
            reservation_coordinator=reservation_coordinator,
            reconcile_context=reconcile_context,
        )
    finally:
        store.close()


def run_cycle_preamble(
    *,
    program_path: str,
    db_path: str,
    coinset_base_url: str,
    poll_coinset_mempool: bool,
    use_websocket_capture: bool,
) -> dict[str, Any]:
    program = load_program_config(Path(program_path))
    store = SqliteStore(Path(db_path))
    cycle_error_count = 0
    xch_price_usd: float | None = None
    try:
        try:
            price = XchPriceProvider(fallback_price_adapter=PriceAdapter())
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
                coinset = _build_coinset_adapter(
                    program=program,
                    coinset_base_url=coinset_base_url,
                )
                tx_ids = coinset.get_all_mempool_tx_ids()
                new_count = store.observe_mempool_tx_ids(tx_ids)
                store.add_audit_event("coinset_mempool_snapshot", {"count": len(tx_ids)})
                if new_count:
                    store.add_audit_event("mempool_observed", {"new_tx_ids": new_count})
            except Exception as exc:  # pragma: no cover - network dependent
                cycle_error_count += 1
                store.add_audit_event("coinset_mempool_error", {"error": str(exc)})
    finally:
        store.close()

    return {
        "cycle_error_count": cycle_error_count,
        "xch_price_usd": xch_price_usd,
    }
