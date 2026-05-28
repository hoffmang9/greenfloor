"""Injectable dispatch hooks (tests patch package-level callables)."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any, Protocol

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.storage.sqlite import SqliteStore


class _ManagedOfferPost(Protocol):
    def __call__(
        self,
        *,
        program: ProgramConfig,
        market: MarketConfig,
        size_base_units: int,
        publish_venue: str,
        runtime_dry_run: bool,
        side: str = "sell",
        program_path: Any = None,
    ) -> dict[str, Any]: ...


class _BuildOfferForAction(Protocol):
    def __call__(
        self,
        *,
        program: ProgramConfig,
        market: MarketConfig,
        action: PlannedAction,
        xch_price_usd: float | None,
        program_path: Any = None,
        keyring_yaml_path: str | None = None,
    ) -> dict[str, Any]: ...


class _ExecuteSingleManagedAction(Protocol):
    def __call__(
        self,
        *,
        program: ProgramConfig,
        market: MarketConfig,
        action: PlannedAction,
        publish_venue: str,
        runtime_dry_run: bool,
        dexie: DexieAdapter,
        managed_offer_post: _ManagedOfferPost,
    ) -> StrategyActionItem: ...


class _ExecuteManagedActionWithRetry(Protocol):
    def __call__(
        self,
        *,
        program: ProgramConfig,
        market: MarketConfig,
        action: PlannedAction,
        publish_venue: str,
        runtime_dry_run: bool,
        dexie: DexieAdapter,
        execute_single_managed_action: _ExecuteSingleManagedAction,
        managed_offer_post: _ManagedOfferPost,
    ) -> StrategyActionItem: ...


class _ExecuteSingleLocalAction(Protocol):
    def __call__(
        self,
        *,
        program: ProgramConfig,
        market: MarketConfig,
        action: PlannedAction,
        xch_price_usd: float | None,
        keyring_yaml_path: str,
        dexie: DexieAdapter,
        splash: Any,
        publish_venue: str,
        store: SqliteStore,
        program_path: Any = None,
        build_offer_for_action: _BuildOfferForAction,
    ) -> StrategyActionItem: ...


@dataclass(frozen=True, slots=True)
class StrategyDispatchHooks:
    resolve_signer_offer_asset_ids_for_reservation: Callable[..., tuple[str, str, str]]
    build_offer_for_action: _BuildOfferForAction
    execute_single_local_action: _ExecuteSingleLocalAction
    managed_offer_post: _ManagedOfferPost
    execute_single_managed_action: _ExecuteSingleManagedAction
    execute_managed_action_with_retry: _ExecuteManagedActionWithRetry

    def managed_action_with_retry(
        self,
        *,
        program: ProgramConfig,
        market: MarketConfig,
        action: PlannedAction,
        publish_venue: str,
        runtime_dry_run: bool,
        dexie: DexieAdapter,
    ) -> StrategyActionItem:
        return self.execute_managed_action_with_retry(
            program=program,
            market=market,
            action=action,
            publish_venue=publish_venue,
            runtime_dry_run=runtime_dry_run,
            dexie=dexie,
            execute_single_managed_action=self.execute_single_managed_action,
            managed_offer_post=self.managed_offer_post,
        )

    def local_action(
        self,
        *,
        program: ProgramConfig,
        market: MarketConfig,
        action: PlannedAction,
        xch_price_usd: float | None,
        keyring_yaml_path: str,
        dexie: DexieAdapter,
        splash: Any,
        publish_venue: str,
        store: SqliteStore,
        program_path: Any = None,
    ) -> StrategyActionItem:
        return self.execute_single_local_action(
            program=program,
            market=market,
            action=action,
            xch_price_usd=xch_price_usd,
            keyring_yaml_path=keyring_yaml_path,
            dexie=dexie,
            splash=splash,
            publish_venue=publish_venue,
            store=store,
            program_path=program_path,
            build_offer_for_action=self.build_offer_for_action,
        )


def hooks_from_module() -> StrategyDispatchHooks:
    """Build hooks from current package exports (honors monkeypatch on strategy_dispatch)."""
    from greenfloor.daemon import strategy_dispatch as pkg

    return StrategyDispatchHooks(
        resolve_signer_offer_asset_ids_for_reservation=(
            pkg.resolve_signer_offer_asset_ids_for_reservation
        ),
        build_offer_for_action=pkg.build_offer_for_action,
        execute_single_local_action=pkg.execute_single_local_action,
        managed_offer_post=pkg.managed_offer_post,
        execute_single_managed_action=pkg.execute_single_managed_action,
        execute_managed_action_with_retry=pkg.execute_managed_action_with_retry,
    )
