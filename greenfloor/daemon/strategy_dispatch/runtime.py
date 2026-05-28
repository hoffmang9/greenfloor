"""Injectable dispatch hooks (tests patch package-level callables)."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.runtime.offer_post_request import ManagedOfferPostResult
from greenfloor.storage.sqlite import SqliteStore


@dataclass(frozen=True, slots=True)
class StrategyDispatchHooks:
    resolve_signer_offer_asset_ids_for_reservation: Callable[..., tuple[str, str, str]]
    build_offer_for_action: Callable[..., dict[str, Any]]
    execute_single_local_action: Callable[..., StrategyActionItem]
    managed_offer_post: Callable[..., ManagedOfferPostResult]
    execute_single_managed_action: Callable[..., StrategyActionItem]
    execute_managed_action_with_retry: Callable[..., StrategyActionItem]

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
        splash: SplashAdapter | None,
        publish_venue: str,
        store: SqliteStore,
        program_path: Path | None = None,
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
