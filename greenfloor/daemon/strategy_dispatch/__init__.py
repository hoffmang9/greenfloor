"""Daemon strategy action dispatch (managed signer + local fallback)."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_dispatch.dispatch_router import (
    StrategyDispatchMode,
    execute_strategy_dispatch,
    resolve_strategy_dispatch_mode,
)
from greenfloor.daemon.strategy_dispatch.local_path import (
    build_offer_for_action,
    execute_single_local_action,
)
from greenfloor.daemon.strategy_dispatch.managed_path import (
    execute_managed_action_with_retry,
    execute_single_managed_action,
    managed_offer_post,
)
from greenfloor.daemon.strategy_dispatch.reservation_helpers import (
    resolve_signer_offer_asset_ids_for_reservation,
)
from greenfloor.daemon.strategy_dispatch.results import StrategyActionResult
from greenfloor.daemon.strategy_dispatch.runtime import StrategyDispatchHooks, hooks_from_module
from greenfloor.storage.sqlite import SqliteStore

__all__ = [
    "StrategyActionResult",
    "StrategyDispatchHooks",
    "StrategyDispatchMode",
    "build_offer_for_action",
    "execute_managed_action_with_retry",
    "execute_single_local_action",
    "execute_single_managed_action",
    "execute_strategy_actions",
    "execute_strategy_dispatch",
    "hooks_from_module",
    "managed_offer_post",
    "resolve_signer_offer_asset_ids_for_reservation",
    "resolve_strategy_dispatch_mode",
]


def execute_strategy_actions(
    *,
    market: MarketConfig,
    strategy_actions: list[PlannedAction],
    runtime_dry_run: bool,
    xch_price_usd: float | None,
    dexie: DexieAdapter,
    splash: SplashAdapter | None = None,
    publish_venue: str = "dexie",
    store: SqliteStore,
    app_network: str = "mainnet",
    signer_key_registry: dict[str, Any] | None = None,
    program: ProgramConfig | None = None,
    reservation_coordinator: AssetReservationCoordinator | None = None,
    hooks: StrategyDispatchHooks | None = None,
) -> StrategyActionResult:
    _ = app_network
    return execute_strategy_dispatch(
        market=market,
        strategy_actions=strategy_actions,
        runtime_dry_run=runtime_dry_run,
        xch_price_usd=xch_price_usd,
        dexie=dexie,
        splash=splash,
        publish_venue=publish_venue,
        store=store,
        signer_key_registry=signer_key_registry,
        program=program,
        reservation_coordinator=reservation_coordinator,
        hooks=hooks,
    )
