"""Sequential strategy action dispatch (managed signer only)."""

from __future__ import annotations

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig, signer_offer_path_configured
from greenfloor.core.cycle import is_managed_worker_transient_error, sequential_action_route
from greenfloor.core.planned_action import PlannedAction
from greenfloor.core.strategy_action_item import StrategyActionItem
from greenfloor.daemon.market_logging import _log_offer_action_timing
from greenfloor.daemon.offer_dispatch.items import action_item
from greenfloor.daemon.strategy_execution import StrategyActionResult, StrategyDispatchHooks
from greenfloor.storage.sqlite import SqliteStore


def execute_actions_sequential(
    *,
    program: ProgramConfig | None,
    market: MarketConfig,
    expanded_actions: list[PlannedAction],
    runtime_dry_run: bool,
    xch_price_usd: float | None,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
    publish_venue: str,
    store: SqliteStore,
    keyring_yaml_path: str,
    hooks: StrategyDispatchHooks,
) -> StrategyActionResult:
    del xch_price_usd, splash, store, keyring_yaml_path
    items: list[StrategyActionItem] = []
    for action in expanded_actions:
        managed_backend_available = program is not None and signer_offer_path_configured(program)
        route = sequential_action_route(
            runtime_dry_run=runtime_dry_run,
            program_present=program is not None,
            managed_backend_available=managed_backend_available,
        )
        if route == "dry_run_planned":
            items.append(action_item(action, status="planned", reason="dry_run", offer_id=None))
            continue
        if route == "skip_no_program":
            items.append(
                action_item(
                    action,
                    status="skipped",
                    reason="managed_offer_post_requires_program_config",
                    offer_id=None,
                )
            )
            continue
        if route == "skip_no_managed_backend":
            items.append(
                action_item(
                    action,
                    status="skipped",
                    reason="signer_backend_required",
                    offer_id=None,
                )
            )
            continue
        if route != "managed":
            items.append(
                action_item(
                    action,
                    status="skipped",
                    reason=f"unsupported_sequential_route:{route}",
                    offer_id=None,
                )
            )
            continue
        assert program is not None
        try:
            item = hooks.execute_managed_action_with_retry(
                program=program,
                market=market,
                action=action,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
                execute_single_managed_action=hooks.execute_single_managed_action,
                managed_offer_post=hooks.managed_offer_post,
            )
        except Exception as exc:
            item = action_item(
                action,
                status="skipped",
                reason=f"managed_action_error:{exc}",
                offer_id=None,
                transient_upstream=is_managed_worker_transient_error(exc),
            )
        _log_offer_action_timing(str(market.market_id), item)
        items.append(item)
    return StrategyActionResult.from_items(
        planned_count=len(expanded_actions),
        action_items=items,
    )
