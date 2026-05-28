"""Daemon strategy action dispatch (managed signer + local fallback)."""

from __future__ import annotations

import concurrent.futures
import time
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import (
    MarketConfig,
    ProgramConfig,
    managed_offer_execution_backend,
    signer_offer_path_configured,
)
from greenfloor.core.cycle import (
    can_parallelize_managed_offers,
    classify_dexie_visibility_outcome,
    classify_managed_post_result,
    count_parallel_transient_failures,
    expand_strategy_actions,
    is_managed_upstream_transient_error,
    is_managed_worker_transient_error,
    is_parallel_dispatch_transient_error,
    managed_retry_sleep_ms,
    parallel_max_workers,
    prepare_parallel_managed_submission_decision,
    reservation_release_status,
    reservation_request_for_managed_offer,
    should_apply_parallel_transient_cooldown,
    should_retry_managed_post,
)
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.cooldowns import (
    _POST_COOLDOWN_UNTIL,
    _cooldown_remaining_ms,
    _post_offer_with_retry,
    _post_retry_config,
    _set_cooldown,
    raise_if_transient_managed_upstream_error,
)
from greenfloor.daemon.inventory_scan import _coinset_spendable_profiles_by_asset
from greenfloor.daemon.market_helpers import _normalize_offer_side, _resolve_quote_asset_for_offer
from greenfloor.daemon.market_logging import (
    _log_market_decision,
    _log_offer_action_timing,
)
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.runtime.offer_build_context import (
    default_program_config_path,
    prepare_offer_build_context,
)
from greenfloor.runtime.offer_execution import build_daemon_action_offer_payload
from greenfloor.runtime.offer_post_request import OfferPostRequest, parse_managed_offer_post_result
from greenfloor.runtime.offer_publish import (
    resolve_quote_price_for_market,
    verify_offer_visible_on_dexie,
)
from greenfloor.runtime.offer_runtime import signer_resolve_offer_asset_ids
from greenfloor.storage.sqlite import SqliteStore


def _action_item(
    action: Any,
    *,
    status: str,
    reason: str,
    offer_id: str | None = None,
    **extra: Any,
) -> StrategyActionItem:
    transient_upstream = bool(extra.pop("transient_upstream", False))
    return StrategyActionItem.from_action(
        action,
        status=status,
        reason=reason,
        side=_normalize_offer_side(getattr(action, "side", "sell")),
        offer_id=offer_id,
        transient_upstream=transient_upstream,
        **extra,
    )


def _parallel_offer_worker_error_item(*, exc: Exception) -> StrategyActionItem:
    return StrategyActionItem.from_worker_error(
        exc=exc,
        transient_upstream=is_managed_worker_transient_error(exc),
    )


def _action_item_from_managed_outcome(
    action: Any,
    outcome: dict[str, Any],
    *,
    offer_id: str | None = None,
    **extra: Any,
) -> StrategyActionItem:
    resolved_offer_id = offer_id
    if resolved_offer_id is None:
        raw_offer_id = outcome.get("offer_id")
        resolved_offer_id = str(raw_offer_id).strip() if raw_offer_id else None
    return _action_item(
        action,
        status=str(outcome["status"]),
        reason=str(outcome["reason"]),
        offer_id=resolved_offer_id or None,
        transient_upstream=bool(outcome.get("transient_upstream", False)),
        **extra,
    )


def _can_parallelize_managed_offers(
    *,
    program: ProgramConfig | None,
    runtime_dry_run: bool,
    reservation_coordinator: AssetReservationCoordinator | None,
) -> bool:
    return can_parallelize_managed_offers(
        signer_path_configured=program is not None and signer_offer_path_configured(program),
        parallelism_enabled=bool(program.runtime_offer_parallelism_enabled)
        if program is not None
        else False,
        runtime_dry_run=runtime_dry_run,
        has_coordinator=reservation_coordinator is not None,
    )


def _build_offer_for_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    xch_price_usd: float | None,
    program_path: Path | None = None,
    keyring_yaml_path: str | None = None,
) -> dict[str, Any]:
    from greenfloor.offer_builder import build_offer

    side = _normalize_offer_side(getattr(action, "side", "sell"))
    resolved_keyring_yaml_path = keyring_yaml_path
    resolved_program_path = default_program_config_path(program, program_path)
    try:
        build_ctx = prepare_offer_build_context(
            program=program,
            market=market,
            program_path=resolved_program_path,
            network=program.app_network,
            keyring_yaml_path=resolved_keyring_yaml_path,
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


def _reservation_wallet_id(program: ProgramConfig) -> str:
    vault = program.vault_config
    if vault is not None:
        launcher_id = str(vault.launcher_id).strip()
        if launcher_id:
            return launcher_id
    return "signer"


def _reservation_request_for_managed_offer(
    *,
    market: Any,
    action: Any,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    fee_asset_id: str,
    fee_amount_mojos: int,
) -> dict[str, int]:
    pricing = market.pricing or {}
    base_multiplier = int(pricing.get("base_unit_mojo_multiplier", 1000))
    quote_multiplier = int(pricing.get("quote_unit_mojo_multiplier", 1000))
    return reservation_request_for_managed_offer(
        side=_normalize_offer_side(getattr(action, "side", "sell")),
        size_base_units=int(action.size),
        base_asset_id=str(resolved_base_asset_id or "").strip(),
        quote_asset_id=str(resolved_quote_asset_id or "").strip(),
        base_unit_mojo_multiplier=base_multiplier,
        quote_unit_mojo_multiplier=quote_multiplier,
        quote_price=float(resolve_quote_price_for_market(market)),
        fee_asset_id=str(fee_asset_id or "").strip(),
        fee_amount_mojos=int(fee_amount_mojos),
    )


def _resolve_signer_offer_asset_ids_for_reservation(
    *,
    program: ProgramConfig,
    market: MarketConfig,
) -> tuple[str, str, str]:
    quote_asset = _resolve_quote_asset_for_offer(
        quote_asset=str(getattr(market, "quote_asset", "")),
        network=str(getattr(program, "app_network", "mainnet")),
    )
    resolved_base_asset_id, resolved_quote_asset_id = signer_resolve_offer_asset_ids(
        program=program,
        base_asset_id=str(getattr(market, "base_asset", "")).strip(),
        quote_asset_id=str(quote_asset).strip(),
    )
    resolved_xch_asset_id, _ = signer_resolve_offer_asset_ids(
        program=program,
        base_asset_id="xch",
        quote_asset_id=str(quote_asset).strip(),
    )
    return resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id


def _managed_offer_post(
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


def _execute_single_managed_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
) -> StrategyActionItem:
    """Execute a single strategy action via the managed signer path."""
    managed_post = _managed_offer_post(
        program=program,
        market=market,
        size_base_units=int(action.size),
        publish_venue=publish_venue,
        runtime_dry_run=runtime_dry_run,
        side=_normalize_offer_side(getattr(action, "side", "sell")),
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
        return _action_item_from_managed_outcome(
            action,
            visibility_outcome,
            offer_id=managed_offer_id or None,
            **timing_fields,
        )
    return _action_item_from_managed_outcome(
        action,
        post_outcome,
        **timing_fields,
    )


def _execute_managed_action_with_retry(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
) -> StrategyActionItem:
    """Execute a single managed action with transient-error retries."""
    attempts_max, backoff_ms, _ = _post_retry_config()
    last_exc: Exception | None = None
    for attempt_index in range(max(1, int(attempts_max))):
        try:
            return _execute_single_managed_action(
                program=program,
                market=market,
                action=action,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
            )
        except Exception as exc:
            last_exc = exc
            if not should_retry_managed_post(
                attempt_index=attempt_index,
                attempts_max=int(attempts_max),
                is_upstream_transient=is_managed_upstream_transient_error(exc),
            ):
                raise
            sleep_ms = managed_retry_sleep_ms(
                attempt_index=attempt_index,
                backoff_ms=int(backoff_ms),
            )
            if sleep_ms > 0:
                time.sleep(float(sleep_ms) / 1000.0)
    raise RuntimeError(str(last_exc or "managed_action_retry_exhausted"))


def _execute_single_local_action(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    action: Any,
    xch_price_usd: float | None,
    keyring_yaml_path: str,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
    publish_venue: str,
    store: SqliteStore,
    program_path: Path | None = None,
) -> StrategyActionItem:
    """Execute a single strategy action via the local build+sign+post path."""
    action_started = time.monotonic()
    build_started = action_started
    built = _build_offer_for_action(
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
        return _action_item(
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
        return _action_item(
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
        return _action_item(
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
    return _action_item(
        action,
        status="skipped",
        reason=f"{publish_venue}_post_retry_exhausted:{post_error}",
        offer_id=offer_id or None,
        attempts=attempt_count,
        offer_create_ms=build_ms,
        offer_publish_ms=publish_ms,
        offer_total_ms=int((time.monotonic() - action_started) * 1000),
    )


def _managed_skip_item(*, action: Any, reason: str) -> StrategyActionItem:
    return _action_item(action, status="skipped", reason=reason, offer_id=None)


def _prepare_parallel_managed_submission(
    *,
    market: Any,
    action: Any,
    program: ProgramConfig,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    resolved_xch_asset_id: str,
    fee_amount_mojos: int,
) -> tuple[dict[str, int] | None, dict[str, int] | None, StrategyActionItem | None]:
    requested_amounts = _reservation_request_for_managed_offer(
        market=market,
        action=action,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
        fee_asset_id=resolved_xch_asset_id,
        fee_amount_mojos=fee_amount_mojos,
    )
    spendable_profiles = _coinset_spendable_profiles_by_asset(
        program=program,
        market=market,
        asset_ids=set(requested_amounts.keys()),
    )
    decision = prepare_parallel_managed_submission_decision(
        requested_amounts=requested_amounts,
        spendable_profiles=spendable_profiles,
    )
    if decision.get("decision") == "skip":
        return (
            None,
            None,
            _managed_skip_item(action=action, reason=str(decision.get("reason", "skipped"))),
        )
    available_amounts = {
        str(asset_id): int(amount)
        for asset_id, amount in dict(decision.get("available_amounts", {})).items()
    }
    return requested_amounts, available_amounts, None


def _strategy_action_result(
    *,
    planned_count: int,
    executed_count: int,
    items: list[StrategyActionItem],
) -> dict[str, Any]:
    return {
        "planned_count": planned_count,
        "executed_count": executed_count,
        "items": [item.to_audit_dict() for item in items],
    }


def _execute_actions_parallel(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    expanded_actions: list[Any],
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
    reservation_coordinator: AssetReservationCoordinator,
) -> dict[str, Any]:
    items: list[StrategyActionItem] = []
    executed_count = 0
    resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id = (
        _resolve_signer_offer_asset_ids_for_reservation(
            program=program,
            market=market,
        )
    )
    # Offer files must always use zero fees; fees apply only to coin split/combine.
    fee_amount_mojos = 0
    wallet_id = _reservation_wallet_id(program)
    reservation_coordinator.probe_storage()
    submissions: list[tuple[int, Any, dict[str, int], dict[str, int]]] = []
    for submit_index, action in enumerate(expanded_actions):
        requested_amounts, available_amounts, skip_item = _prepare_parallel_managed_submission(
            market=market,
            action=action,
            program=program,
            resolved_base_asset_id=resolved_base_asset_id,
            resolved_quote_asset_id=resolved_quote_asset_id,
            resolved_xch_asset_id=resolved_xch_asset_id,
            fee_amount_mojos=fee_amount_mojos,
        )
        if skip_item is not None:
            items.append(skip_item)
            continue
        assert requested_amounts is not None
        assert available_amounts is not None
        submissions.append((submit_index, action, requested_amounts, available_amounts))

    if not submissions:
        return _strategy_action_result(
            planned_count=len(expanded_actions),
            executed_count=executed_count,
            items=items,
        )

    max_workers = parallel_max_workers(
        submission_count=len(submissions),
        configured_max=int(program.runtime_offer_parallelism_max_workers),
    )
    _log_market_decision(
        str(market.market_id),
        "parallel_offer_dispatch",
        planned_count=len(expanded_actions),
        queued_count=len(submissions),
        workers=max_workers,
    )

    def _run_parallel_submission(
        *,
        submit_index: int,
        action: Any,
        requested_amounts: dict[str, int],
        available_amounts: dict[str, int],
        queued_at_monotonic: float,
    ) -> StrategyActionItem:
        queue_wait_ms = int((time.monotonic() - queued_at_monotonic) * 1000)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_queue_wait",
            submit_index=submit_index,
            size=int(getattr(action, "size", 0)),
            side=_normalize_offer_side(getattr(action, "side", "sell")),
            queue_wait_ms=queue_wait_ms,
        )
        acquire_started = time.monotonic()
        acquired = reservation_coordinator.try_acquire(
            market_id=str(market.market_id),
            wallet_id=wallet_id,
            requested_amounts=requested_amounts,
            available_amounts=available_amounts,
        )
        acquire_ms = int((time.monotonic() - acquire_started) * 1000)
        if not acquired.ok or not acquired.reservation_id:
            return _managed_skip_item(
                action=action,
                reason=str(acquired.error or "reservation_rejected"),
            ).with_extra(
                queue_wait_ms=queue_wait_ms,
                reservation_acquire_ms=acquire_ms,
            )
        reservation_id = str(acquired.reservation_id)
        reserved_at = time.monotonic()
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_reservation_acquired",
            submit_index=submit_index,
            reservation_id=reservation_id,
            queue_wait_ms=queue_wait_ms,
            reservation_acquire_ms=acquire_ms,
        )
        try:
            item = _execute_managed_action_with_retry(
                program=program,
                market=market,
                action=action,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
            )
        except Exception as exc:
            item = _parallel_offer_worker_error_item(exc=exc)
        release_status = reservation_release_status(is_executed=item.is_executed)
        reservation_coordinator.release(reservation_id=reservation_id, status=release_status)
        reservation_hold_ms = int((time.monotonic() - reserved_at) * 1000)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_reservation_released",
            submit_index=submit_index,
            reservation_id=reservation_id,
            release_status=release_status,
            reservation_hold_ms=reservation_hold_ms,
        )
        return item.with_extra(
            reservation_id=reservation_id,
            queue_wait_ms=queue_wait_ms,
            reservation_acquire_ms=acquire_ms,
            reservation_hold_ms=reservation_hold_ms,
        )

    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
        future_to_submission: dict[concurrent.futures.Future[StrategyActionItem], int] = {}
        for submit_index, action, requested_amounts, available_amounts in submissions:
            future = pool.submit(
                _run_parallel_submission,
                submit_index=submit_index,
                action=action,
                requested_amounts=requested_amounts,
                available_amounts=available_amounts,
                queued_at_monotonic=time.monotonic(),
            )
            future_to_submission[future] = submit_index
        submitted_items: list[tuple[int, StrategyActionItem]] = []
        for future in concurrent.futures.as_completed(future_to_submission):
            submit_index = future_to_submission[future]
            try:
                item = future.result()
            except Exception as exc:
                item = _parallel_offer_worker_error_item(exc=exc)
            submitted_items.append((submit_index, item))
        for _, item in sorted(submitted_items, key=lambda pair: pair[0]):
            _log_offer_action_timing(str(market.market_id), item)
            if item.is_executed:
                executed_count += 1
            items.append(item)

    _, _, cooldown_seconds = _post_retry_config()
    transient_parallel_failures = count_parallel_transient_failures(
        [
            {
                "status": item.status,
                "transient_upstream": item.transient_upstream,
            }
            for _submit_idx, item in submitted_items
        ]
    )
    total_parallel = len(submitted_items)
    if should_apply_parallel_transient_cooldown(
        transient_failures=transient_parallel_failures,
        total_parallel=total_parallel,
        cooldown_seconds=int(cooldown_seconds),
    ):
        cooldown_key = f"{publish_venue}:{market.market_id}"
        _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
        _log_market_decision(
            str(market.market_id),
            "parallel_offer_transient_cooldown",
            transient_failures=transient_parallel_failures,
            total_parallel=total_parallel,
            cooldown_seconds=cooldown_seconds,
        )
    return _strategy_action_result(
        planned_count=len(expanded_actions),
        executed_count=executed_count,
        items=items,
    )


def _execute_actions_sequential(
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
    items: list[StrategyActionItem] = []
    executed_count = 0
    for action in expanded_actions:
        if runtime_dry_run:
            items.append(_action_item(action, status="planned", reason="dry_run", offer_id=None))
            continue
        backend = (
            managed_offer_execution_backend(program, size_base_units=int(action.size))
            if program is not None
            else None
        )
        if backend is not None:
            assert program is not None
            try:
                item = _execute_managed_action_with_retry(
                    program=program,
                    market=market,
                    action=action,
                    publish_venue=publish_venue,
                    runtime_dry_run=runtime_dry_run,
                    dexie=dexie,
                )
            except Exception as exc:
                item = _action_item(
                    action,
                    status="skipped",
                    reason=f"managed_action_error:{exc}",
                    offer_id=None,
                    transient_upstream=is_managed_worker_transient_error(exc),
                )
        elif program is None:
            item = _action_item(
                action,
                status="skipped",
                reason="local_offer_post_requires_program_config",
                offer_id=None,
            )
        else:
            item = _execute_single_local_action(
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
    return _strategy_action_result(
        planned_count=len(expanded_actions),
        executed_count=executed_count,
        items=items,
    )


def _execute_strategy_actions(
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
) -> dict[str, Any]:
    _ = app_network
    signer_key_id = str(market.signer_key_id or "").strip()
    signer_key = (signer_key_registry or {}).get(signer_key_id)
    if isinstance(signer_key, dict):
        keyring_yaml_path = str(signer_key.get("keyring_yaml_path", "") or "").strip()
    else:
        keyring_yaml_path = str(getattr(signer_key, "keyring_yaml_path", "") or "").strip()
    expanded_actions = expand_strategy_actions(strategy_actions)
    if _can_parallelize_managed_offers(
        program=program,
        runtime_dry_run=runtime_dry_run,
        reservation_coordinator=reservation_coordinator,
    ):
        assert program is not None
        assert reservation_coordinator is not None
        try:
            return _execute_actions_parallel(
                program=program,
                market=market,
                expanded_actions=expanded_actions,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
                reservation_coordinator=reservation_coordinator,
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
    return _execute_actions_sequential(
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
    )
