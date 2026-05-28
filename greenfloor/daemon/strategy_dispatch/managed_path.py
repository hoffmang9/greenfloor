"""Managed signer strategy action execution."""

from __future__ import annotations

import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig, managed_offer_execution_backend
from greenfloor.core.cycle import (
    classify_dexie_visibility_outcome,
    classify_managed_post_result,
    is_managed_upstream_transient_error,
    managed_retry_decision,
)
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.cooldowns import (
    _post_retry_config,
    raise_if_transient_managed_upstream_error,
)
from greenfloor.daemon.market_helpers import _normalize_offer_side
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.strategy_dispatch.items import action_item_from_managed_outcome
from greenfloor.runtime.offer_build_context import (
    default_program_config_path,
    prepare_offer_build_context,
)
from greenfloor.runtime.offer_post_request import OfferPostRequest, parse_managed_offer_post_result
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
) -> dict[str, Any]:
    backend = managed_offer_execution_backend(program, size_base_units=size_base_units)
    if backend is None:
        return {
            "success": False,
            "error": "managed_offer_post_requires_signer_backend",
        }

    build_ctx = prepare_offer_build_context(
        program=program,
        market=market,
        program_path=default_program_config_path(program, program_path),
        network=program.app_network,
        action_side=side,
    )
    request = OfferPostRequest(
        build_ctx=build_ctx,
        size_base_units=size_base_units,
        repeat=1,
        publish_venue=publish_venue,
        dexie_base_url=str(program.dexie_api_base),
        splash_base_url=str(program.splash_api_base),
        drop_only=True,
        claim_rewards=False,
        dry_run=runtime_dry_run,
    )
    exit_code, payload = request.run_managed(backend)
    result = parse_managed_offer_post_result(exit_code, payload)
    if not bool(result.get("success", False)):
        error_text = str(result.get("error", "")).strip()
        if error_text:
            raise_if_transient_managed_upstream_error(error_text)
    return result


def execute_single_managed_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: PlannedAction,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    managed_offer_post: Callable[..., dict[str, Any]],
) -> StrategyActionItem:
    managed_post = managed_offer_post(
        program=program,
        market=market,
        size_base_units=action.size,
        publish_venue=publish_venue,
        runtime_dry_run=runtime_dry_run,
        side=_normalize_offer_side(action.side),
    )
    timing_fields = {
        "offer_create_ms": managed_post.get("offer_create_ms"),
        "offer_publish_ms": managed_post.get("offer_publish_ms"),
        "offer_total_ms": managed_post.get("offer_total_ms"),
        "offer_create_phase_ms": managed_post.get("offer_create_phase_ms"),
        "offer_artifact_wait_ms": managed_post.get("offer_artifact_wait_ms"),
    }
    post_outcome = classify_managed_post_result(
        success=bool(managed_post.get("success", False)),
        error_text=str(managed_post.get("error", "unknown")),
        offer_id=str(managed_post.get("offer_id", "")),
        publish_venue=publish_venue,
    )
    if post_outcome.get("status") == "pending_visibility":
        managed_offer_id = str(managed_post.get("offer_id", "")).strip()
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
            **timing_fields,
        )
    return action_item_from_managed_outcome(
        action,
        post_outcome,
        **timing_fields,
    )


def execute_managed_action_with_retry(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: PlannedAction,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    execute_single_managed_action: Callable[..., StrategyActionItem],
    managed_offer_post: Callable[..., dict[str, Any]],
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
