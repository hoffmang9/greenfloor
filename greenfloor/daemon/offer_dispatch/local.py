"""Local BLS build+post strategy action execution."""

from __future__ import annotations

import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.core.offer_side import normalize_offer_side
from greenfloor.core.planned_action import PlannedAction
from greenfloor.core.strategy_action_item import StrategyActionItem
from greenfloor.daemon.cooldowns import (
    _POST_COOLDOWN_UNTIL,
    _cooldown_remaining_ms,
    _post_offer_with_retry,
    _post_retry_config,
    _set_cooldown,
)
from greenfloor.daemon.offer_dispatch.items import action_item
from greenfloor.runtime.offer_build_context import (
    default_program_config_path,
    prepare_offer_build_context,
)
from greenfloor.runtime.offer_execution import build_daemon_action_offer_payload
from greenfloor.storage.sqlite import SqliteStore


def build_offer_for_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: PlannedAction,
    xch_price_usd: float | None,
    program_path: Path | None = None,
    keyring_yaml_path: str | None = None,
) -> dict[str, Any]:
    from greenfloor.offer_builder import build_offer

    side = normalize_offer_side(action.side)
    resolved_program_path = default_program_config_path(program, program_path)
    try:
        build_ctx = prepare_offer_build_context(
            program=program,
            market=market,
            program_path=resolved_program_path,
            network=program.app_network,
            keyring_yaml_path=keyring_yaml_path,
            action_side=side,
        )
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"offer_builder_failed:{exc}",
            "offer": None,
        }
    payload = build_daemon_action_offer_payload(
        build_ctx,
        action=action,
        xch_price_usd=xch_price_usd,
    )
    try:
        offer = build_offer(payload)
    except Exception as exc:
        return {"status": "skipped", "reason": f"offer_builder_failed:{exc}", "offer": None}
    return {"status": "executed", "reason": "offer_builder_success", "offer": offer}


def execute_single_local_action(
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
    build_offer_for_action: Callable[..., dict[str, Any]],
) -> StrategyActionItem:
    action_started = time.monotonic()
    build_started = action_started
    built = build_offer_for_action(
        program=program,
        market=market,
        action=action,
        xch_price_usd=xch_price_usd,
        program_path=program_path,
        keyring_yaml_path=keyring_yaml_path,
    )
    build_ms = int((time.monotonic() - build_started) * 1000)
    if built.get("status") != "executed":
        built_reason = str(built.get("reason", "offer_builder_skipped"))
        return action_item(
            action,
            status="skipped",
            reason=built_reason,
            offer_id=None,
            offer_create_ms=build_ms,
            offer_publish_ms=None,
            offer_total_ms=int((time.monotonic() - action_started) * 1000),
        )
    _, _, cooldown_seconds = _post_retry_config()
    cooldown_key = f"{publish_venue}:{market.market_id}"
    remaining_ms = _cooldown_remaining_ms(_POST_COOLDOWN_UNTIL, cooldown_key)
    if remaining_ms > 0:
        return action_item(
            action,
            status="skipped",
            reason=f"post_cooldown_active:{remaining_ms}ms",
            offer_id=None,
            offer_create_ms=build_ms,
            offer_publish_ms=None,
            offer_total_ms=int((time.monotonic() - action_started) * 1000),
        )
    offer_text = str(built["offer"])
    publish_started = time.monotonic()
    post_result, attempt_count, post_error = _post_offer_with_retry(
        publish_venue=publish_venue,
        offer_text=offer_text,
        dexie=dexie,
        splash=splash,
    )
    publish_ms = int((time.monotonic() - publish_started) * 1000)
    success = bool(post_result.get("success", False))
    offer_id_raw = post_result.get("id")
    offer_id = str(offer_id_raw).strip() if offer_id_raw is not None else ""
    if success and offer_id:
        store.upsert_offer_state(
            offer_id=offer_id,
            market_id=market.market_id,
            state=OfferLifecycleState.OPEN.value,
            last_seen_status=0,
        )
        return action_item(
            action,
            status="executed",
            reason=f"{publish_venue}_post_success",
            offer_id=offer_id,
            attempts=attempt_count,
            offer_create_ms=build_ms,
            offer_publish_ms=publish_ms,
            offer_total_ms=int((time.monotonic() - action_started) * 1000),
        )
    _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
    return action_item(
        action,
        status="skipped",
        reason=f"{publish_venue}_post_retry_exhausted:{post_error}",
        offer_id=offer_id or None,
        attempts=attempt_count,
        offer_create_ms=build_ms,
        offer_publish_ms=publish_ms,
        offer_total_ms=int((time.monotonic() - action_started) * 1000),
    )
