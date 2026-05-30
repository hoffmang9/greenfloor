"""Managed signer strategy action execution."""

from __future__ import annotations

import time
from collections.abc import Callable
from pathlib import Path

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig, require_signer_offer_path
from greenfloor.core.cycle import (
    classify_dexie_visibility_outcome,
    classify_managed_post_result,
    is_managed_upstream_transient_error,
    managed_retry_decision,
)
from greenfloor.core.planned_action import PlannedAction, planned_action_side
from greenfloor.core.strategy_action_item import StrategyActionItem
from greenfloor.daemon.cooldowns import (
    _post_retry_config,
    raise_if_transient_managed_upstream_error,
)
from greenfloor.daemon.offer_dispatch.items import action_item_from_managed_outcome
from greenfloor.runtime.daemon_config_paths import resolve_daemon_config_paths
from greenfloor.runtime.engine_build_and_post import run_build_and_post_offer_in_process
from greenfloor.runtime.offer_post_request import (
    ManagedOfferPostResult,
    parse_managed_offer_post_result,
)
from greenfloor.runtime.offer_publish import verify_offer_visible_on_dexie


def managed_offer_post(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    size_base_units: int,
    publish_venue: str,
    runtime_dry_run: bool,
    side: str = "sell",
    program_path: Path | None = None,
) -> ManagedOfferPostResult:
    require_signer_offer_path(program)

    paths = resolve_daemon_config_paths(program, program_path)
    exit_code, payload = run_build_and_post_offer_in_process(
        paths=paths,
        network=program.app_network,
        market_id=str(market.market_id),
        size_base_units=size_base_units,
        publish_venue=publish_venue,
        dexie_base_url=str(program.dexie_api_base),
        splash_base_url=str(program.splash_api_base),
        drop_only=True,
        claim_rewards=False,
        dry_run=runtime_dry_run,
        action_side=side,
        persist_results=False,
    )
    result = parse_managed_offer_post_result(exit_code, payload)
    if not result.success and result.error:
        raise_if_transient_managed_upstream_error(result.error)
    return result


def execute_single_managed_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: PlannedAction,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    managed_offer_post: Callable[..., ManagedOfferPostResult],
) -> StrategyActionItem:
    managed_post = managed_offer_post(
        program=program,
        market=market,
        size_base_units=action.size,
        publish_venue=publish_venue,
        runtime_dry_run=runtime_dry_run,
        side=planned_action_side(action),
    )
    timing_fields = managed_post.timing_extra()
    post_outcome = classify_managed_post_result(
        success=managed_post.success,
        error_text=managed_post.error or "unknown",
        offer_id=managed_post.offer_id or "",
        publish_venue=publish_venue,
    )
    if not post_outcome.is_pending_visibility:
        return action_item_from_managed_outcome(action, post_outcome).with_extra(**timing_fields)
    managed_offer_id = (post_outcome.offer_id or "").strip()
    visible, visibility_error = verify_offer_visible_on_dexie(
        dexie=dexie,
        offer_id=managed_offer_id,
    )
    visibility_outcome = classify_dexie_visibility_outcome(
        visible=visible,
        visibility_error=visibility_error or "",
    )
    return action_item_from_managed_outcome(
        action,
        visibility_outcome,
        offer_id=managed_offer_id or None,
    ).with_extra(**timing_fields)


def execute_managed_action_with_retry(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: PlannedAction,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    execute_single_managed_action: Callable[..., StrategyActionItem],
    managed_offer_post: Callable[..., ManagedOfferPostResult],
) -> StrategyActionItem:
    attempts_max, backoff_ms, _ = _post_retry_config()
    last_exc: Exception | None = None
    for attempt_index in range(max(1, int(attempts_max))):
        try:
            return execute_single_managed_action(
                program=program,
                market=market,
                action=action,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
                managed_offer_post=managed_offer_post,
            )
        except Exception as exc:
            last_exc = exc
            retry = managed_retry_decision(
                attempt_index=attempt_index,
                attempts_max=int(attempts_max),
                backoff_ms=int(backoff_ms),
                is_upstream_transient=is_managed_upstream_transient_error(exc),
            )
            if not retry.should_retry:
                raise
            if retry.sleep_ms > 0:
                time.sleep(float(retry.sleep_ms) / 1000.0)
    raise RuntimeError(str(last_exc or "managed_action_retry_exhausted"))
