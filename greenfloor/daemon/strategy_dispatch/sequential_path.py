"""Sequential strategy action dispatch (managed or local)."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig, managed_offer_execution_backend
from greenfloor.core.cycle import is_managed_worker_transient_error, sequential_action_route
from greenfloor.daemon.market_logging import _log_offer_action_timing
from greenfloor.daemon.strategy_dispatch.items import action_item, strategy_action_result
from greenfloor.storage.sqlite import SqliteStore


def execute_actions_sequential(
    *,
    program: ProgramConfig | None,
    market: MarketConfig,
    expanded_actions: list[Any],
    runtime_dry_run: bool,
    xch_price_usd: float | None,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
    publish_venue: str,
    store: SqliteStore,
    keyring_yaml_path: str,
) -> dict[str, Any]:
    from greenfloor.daemon import strategy_dispatch as dispatch_pkg

    items = []
    executed_count = 0
    for action in expanded_actions:
        managed_backend_available = (
            managed_offer_execution_backend(program, size_base_units=int(action.size))
            is not None
            if program is not None
            else False
        )
        route = sequential_action_route(
            runtime_dry_run=runtime_dry_run,
            program_present=program is not None,
            managed_backend_available=managed_backend_available,
        )
        if route == "dry_run_planned":
            items.append(action_item(action, status="planned", reason="dry_run", offer_id=None))
            continue
        if route == "managed":
            assert program is not None
            try:
                item = dispatch_pkg._execute_managed_action_with_retry(
                    program=program,
                    market=market,
                    action=action,
                    publish_venue=publish_venue,
                    runtime_dry_run=runtime_dry_run,
                    dexie=dexie,
                )
            except Exception as exc:
                item = action_item(
                    action,
                    status="skipped",
                    reason=f"managed_action_error:{exc}",
                    offer_id=None,
                    transient_upstream=is_managed_worker_transient_error(exc),
                )
        elif route == "skip_no_program":
            item = action_item(
                action,
                status="skipped",
                reason="local_offer_post_requires_program_config",
                offer_id=None,
            )
        else:
            assert program is not None
            item = dispatch_pkg._execute_single_local_action(
                program=program,
                market=market,
                action=action,
                xch_price_usd=xch_price_usd,
                keyring_yaml_path=keyring_yaml_path,
                dexie=dexie,
                splash=splash,
                publish_venue=publish_venue,
                store=store,
            )
        if item.is_executed:
            executed_count += 1
        _log_offer_action_timing(str(market.market_id), item)
        items.append(item)
    return strategy_action_result(
        planned_count=len(expanded_actions),
        executed_count=executed_count,
        items=items,
    )
