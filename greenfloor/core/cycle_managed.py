"""Rust-backed managed offer dispatch policy (transient errors, retry, parallel gating)."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters import cycle_kernel


def is_transient_managed_upstream_error_text(error_text: str) -> bool:
    return cycle_kernel.is_transient_managed_upstream_error_text(error_text)


def classify_managed_transient_error(exc: BaseException) -> str | None:
    return cycle_kernel.classify_managed_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_upstream_transient_error(exc: BaseException) -> bool:
    return cycle_kernel.is_managed_upstream_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_managed_worker_transient_error(exc: BaseException) -> bool:
    return cycle_kernel.is_managed_worker_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_parallel_dispatch_transient_error(exc: BaseException) -> bool:
    return cycle_kernel.is_parallel_dispatch_transient_error(
        exception_class=type(exc).__name__,
        error_text=str(exc),
    )


def is_transient_dexie_visibility_404_error(error: str) -> bool:
    return cycle_kernel.is_transient_dexie_visibility_404_error(error)


def can_parallelize_managed_offers(
    *,
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> bool:
    return cycle_kernel.can_parallelize_managed_offers(
        signer_path_configured=signer_path_configured,
        parallelism_enabled=parallelism_enabled,
        runtime_dry_run=runtime_dry_run,
        has_coordinator=has_coordinator,
    )


def parallel_max_workers(*, submission_count: int, configured_max: int) -> int:
    return cycle_kernel.parallel_max_workers(
        submission_count=submission_count,
        configured_max=configured_max,
    )


def reservation_release_status(*, is_executed: bool) -> str:
    return cycle_kernel.reservation_release_status(is_executed=is_executed)


def should_apply_parallel_transient_cooldown(
    *,
    transient_failures: int,
    total_parallel: int,
    cooldown_seconds: int,
) -> bool:
    return cycle_kernel.should_apply_parallel_transient_cooldown(
        transient_failures=transient_failures,
        total_parallel=total_parallel,
        cooldown_seconds=cooldown_seconds,
    )


def managed_retry_sleep_ms(*, attempt_index: int, backoff_ms: int) -> int:
    return cycle_kernel.managed_retry_sleep_ms(
        attempt_index=attempt_index,
        backoff_ms=backoff_ms,
    )


def should_retry_managed_post(
    *,
    attempt_index: int,
    attempts_max: int,
    is_upstream_transient: bool,
) -> bool:
    return cycle_kernel.should_retry_managed_post(
        attempt_index=attempt_index,
        attempts_max=attempts_max,
        is_upstream_transient=is_upstream_transient,
    )


def prepare_parallel_managed_submission_decision(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> dict[str, Any]:
    return cycle_kernel.prepare_parallel_managed_submission_decision(
        requested_amounts=requested_amounts,
        spendable_profiles=spendable_profiles,
    )


def classify_managed_post_result(
    *,
    success: bool,
    error_text: str,
    offer_id: str,
    publish_venue: str,
) -> dict[str, Any]:
    return cycle_kernel.classify_managed_post_result(
        success=success,
        error_text=error_text,
        offer_id=offer_id,
        publish_venue=publish_venue,
    )


def classify_dexie_visibility_outcome(
    *,
    visible: bool,
    visibility_error: str,
) -> dict[str, Any]:
    return cycle_kernel.classify_dexie_visibility_outcome(
        visible=visible,
        visibility_error=visibility_error,
    )


def count_parallel_transient_failures(items: list[dict[str, Any]]) -> int:
    return cycle_kernel.count_parallel_transient_failures(items)
