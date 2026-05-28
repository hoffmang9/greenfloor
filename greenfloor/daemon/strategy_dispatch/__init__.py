"""Daemon strategy action dispatch (managed signer + local fallback)."""

from __future__ import annotations

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
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_dispatch.local_path import (
    build_offer_for_action,
    execute_single_local_action,
)
from greenfloor.daemon.strategy_dispatch.managed_path import (
    execute_managed_action_with_retry,
    execute_single_managed_action,
    managed_offer_post,
)
from greenfloor.daemon.strategy_dispatch.parallel_managed import execute_actions_parallel
from greenfloor.daemon.strategy_dispatch.reservation_helpers import (
    resolve_signer_offer_asset_ids_for_reservation,
)
from greenfloor.daemon.strategy_dispatch.runtime import StrategyDispatchHooks, hooks_from_module
from greenfloor.daemon.strategy_dispatch.sequential_path import execute_actions_sequential
from greenfloor.storage.sqlite import SqliteStore

__all__ = [
    "StrategyDispatchHooks",
    "build_offer_for_action",
    "execute_managed_action_with_retry",
    "execute_single_local_action",
    "execute_single_managed_action",
    "execute_strategy_actions",
    "hooks_from_module",
    "managed_offer_post",
    "resolve_signer_offer_asset_ids_for_reservation",
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
) -> dict[str, Any]:
    _ = app_network
    dispatch_hooks = hooks or hooks_from_module()
    signer_key_id = str(market.signer_key_id or "").strip()
    signer_key = (signer_key_registry or {}).get(signer_key_id)
    if isinstance(signer_key, dict):
        keyring_yaml_path = str(signer_key.get("keyring_yaml_path", "") or "").strip()
    else:
        keyring_yaml_path = str(getattr(signer_key, "keyring_yaml_path", "") or "").strip()
    expanded_actions = expand_planned_actions(strategy_actions)
    use_parallel = can_parallelize_managed_offers(
        signer_path_configured=program is not None and signer_offer_path_configured(program),
        parallelism_enabled=bool(program.runtime_offer_parallelism_enabled)
        if program is not None
        else False,
        runtime_dry_run=runtime_dry_run,
        has_coordinator=reservation_coordinator is not None,
    )
    if use_parallel:
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
            store.add_audit_event(
                "offer_parallel_fallback",
                {
                    "market_id": str(market.market_id),
                    "error": str(exc),
                    "reason": "reservation_parallel_path_failed",
                },
                market_id=str(market.market_id),
            )
    return execute_actions_sequential(
        program=program,
        market=market,
        expanded_actions=expanded_actions,
        runtime_dry_run=runtime_dry_run,
        xch_price_usd=xch_price_usd,
        dexie=dexie,
        splash=splash,
        publish_venue=publish_venue,
        store=store,
        keyring_yaml_path=keyring_yaml_path,
        hooks=dispatch_hooks,
    )
