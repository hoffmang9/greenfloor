"""Explicit strategy action dispatch routing (parallel vs sequential)."""

from __future__ import annotations

from enum import Enum
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig, signer_offer_path_configured
from greenfloor.core.cycle import (
    can_parallelize_managed_offers,
    expand_planned_actions,
    is_parallel_dispatch_transient_error,
)
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.offer_dispatch.parallel import execute_actions_parallel
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.offer_dispatch.sequential import execute_actions_sequential
from greenfloor.daemon.strategy_execution import (
    StrategyActionResult,
    StrategyDispatchHooks,
    hooks_from_module,
)
from greenfloor.storage.sqlite import SqliteStore


class StrategyDispatchMode(str, Enum):
    PARALLEL = "parallel"
    SEQUENTIAL = "sequential"


def resolve_strategy_dispatch_mode(
    *,
    program: ProgramConfig | None,
    runtime_dry_run: bool,
    reservation_coordinator: AssetReservationCoordinator | None,
) -> StrategyDispatchMode:
    if can_parallelize_managed_offers(
        signer_path_configured=program is not None and signer_offer_path_configured(program),
        parallelism_enabled=bool(program.runtime_offer_parallelism_enabled)
        if program is not None
        else False,
        runtime_dry_run=runtime_dry_run,
        has_coordinator=reservation_coordinator is not None,
    ):
        return StrategyDispatchMode.PARALLEL
    return StrategyDispatchMode.SEQUENTIAL


def _resolve_keyring_yaml_path(
    *,
    market: MarketConfig,
    signer_key_registry: dict[str, Any] | None,
) -> str:
    signer_key_id = str(market.signer_key_id or "").strip()
    signer_key = (signer_key_registry or {}).get(signer_key_id)
    if isinstance(signer_key, dict):
        return str(signer_key.get("keyring_yaml_path", "") or "").strip()
    return str(getattr(signer_key, "keyring_yaml_path", "") or "").strip()


def _record_parallel_fallback(
    *,
    store: SqliteStore,
    market: MarketConfig,
    exc: Exception,
) -> None:
    store.add_audit_event(
        "offer_parallel_fallback",
        {
            "market_id": str(market.market_id),
            "error": str(exc),
            "reason": "reservation_parallel_path_failed",
        },
        market_id=str(market.market_id),
    )


def execute_strategy_dispatch(
    *,
    market: MarketConfig,
    strategy_actions: list[PlannedAction],
    runtime_dry_run: bool,
    xch_price_usd: float | None,
    dexie: DexieAdapter,
    store: SqliteStore,
    splash: SplashAdapter | None = None,
    publish_venue: str = "dexie",
    signer_key_registry: dict[str, Any] | None = None,
    program: ProgramConfig | None = None,
    reservation_coordinator: AssetReservationCoordinator | None = None,
    hooks: StrategyDispatchHooks | None = None,
) -> StrategyActionResult:
    dispatch_hooks = hooks or hooks_from_module()
    expanded_actions = expand_planned_actions(strategy_actions)
    keyring_yaml_path = _resolve_keyring_yaml_path(
        market=market,
        signer_key_registry=signer_key_registry,
    )
    sequential_kwargs = {
        "program": program,
        "market": market,
        "expanded_actions": expanded_actions,
        "runtime_dry_run": runtime_dry_run,
        "xch_price_usd": xch_price_usd,
        "dexie": dexie,
        "splash": splash,
        "publish_venue": publish_venue,
        "store": store,
        "keyring_yaml_path": keyring_yaml_path,
        "hooks": dispatch_hooks,
    }

    mode = resolve_strategy_dispatch_mode(
        program=program,
        runtime_dry_run=runtime_dry_run,
        reservation_coordinator=reservation_coordinator,
    )
    if mode == StrategyDispatchMode.PARALLEL:
        assert program is not None
        assert reservation_coordinator is not None
        try:
            return execute_actions_parallel(
                program=program,
                market=market,
                expanded_actions=expanded_actions,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
                reservation_coordinator=reservation_coordinator,
                hooks=dispatch_hooks,
            )
        except Exception as exc:
            if not is_parallel_dispatch_transient_error(exc):
                raise
            _record_parallel_fallback(store=store, market=market, exc=exc)

    return execute_actions_sequential(**sequential_kwargs)
