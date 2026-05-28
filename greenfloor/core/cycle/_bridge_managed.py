"""Managed-offer dispatch PyO3 bridge wrappers."""

from __future__ import annotations

from greenfloor.core.cycle._bridge_common import normalize_spendable_profiles
from greenfloor.core.kernel_bridge import policy_kernel
from greenfloor.core.managed_action_outcome import ManagedActionOutcome
from greenfloor.core.managed_retry import ManagedRetryDecision
from greenfloor.core.parallel_batch_plan import ParallelBatchPlan
from greenfloor.core.parallel_reservation_context import ParallelReservationContext
from greenfloor.core.planned_action import PlannedAction, planned_actions_from_signer_list
from greenfloor.daemon.strategy_action_item import StrategyActionItem

__all__ = [
    "can_parallelize_managed_offers",
    "classify_dexie_visibility_outcome",
    "classify_managed_post_result",
    "classify_managed_transient_error",
    "count_parallel_transient_failures",
    "expand_planned_actions",
    "filter_planned_actions_with_positive_repeat",
    "is_managed_upstream_transient_error",
    "is_managed_worker_transient_error",
    "is_parallel_dispatch_transient_error",
    "is_transient_dexie_visibility_404_error",
    "is_transient_managed_upstream_error_text",
    "managed_retry_decision",
    "parallel_max_workers",
    "plan_parallel_managed_dispatch",
    "reservation_release_status",
    "sequential_action_route",
    "should_apply_parallel_transient_cooldown",
    "single_input_preferred_skip_reason",
]


def sequential_action_route(
    *,
    runtime_dry_run: bool,
    program_present: bool,
    managed_backend_available: bool,
) -> str:
    return str(
        policy_kernel().sequential_action_route(
            bool(runtime_dry_run),
            bool(program_present),
            bool(managed_backend_available),
        )
    )


def expand_planned_actions(actions: list[PlannedAction]) -> list[PlannedAction]:
    signer = policy_kernel()
    return planned_actions_from_signer_list(signer.expand_planned_actions(actions))


def filter_planned_actions_with_positive_repeat(
    actions: list[PlannedAction],
) -> list[PlannedAction]:
    signer = policy_kernel()
    return planned_actions_from_signer_list(
        signer.filter_planned_actions_with_positive_repeat(actions)
    )


def plan_parallel_managed_dispatch(
    *,
    actions: list[PlannedAction],
    ctx: ParallelReservationContext,
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> ParallelBatchPlan:
    signer = policy_kernel()
    result = signer.plan_parallel_managed_dispatch(
        actions,
        ctx,
        normalize_spendable_profiles(spendable_profiles),
    )
    if not isinstance(result, ParallelBatchPlan):
        raise TypeError("plan_parallel_managed_dispatch returned non-ParallelBatchPlan result")
    return result


def single_input_preferred_skip_reason(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> str | None:
    signer = policy_kernel()
    return signer.single_input_preferred_skip_reason(
        requested_amounts,
        normalize_spendable_profiles(spendable_profiles),
    )


def is_transient_managed_upstream_error_text(error_text: str) -> bool:
    return bool(policy_kernel().is_transient_managed_upstream_error_text(error_text))


def classify_managed_transient_error(*, exception_class: str, error_text: str) -> str | None:
    return policy_kernel().classify_managed_transient_error(exception_class, error_text)


def is_managed_upstream_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(policy_kernel().is_managed_upstream_transient_error(exception_class, error_text))


def is_managed_worker_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(policy_kernel().is_managed_worker_transient_error(exception_class, error_text))


def is_parallel_dispatch_transient_error(*, exception_class: str, error_text: str) -> bool:
    return bool(policy_kernel().is_parallel_dispatch_transient_error(exception_class, error_text))


def is_transient_dexie_visibility_404_error(error: str) -> bool:
    return bool(policy_kernel().is_transient_dexie_visibility_404_error(error))


def can_parallelize_managed_offers(
    *,
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> bool:
    return bool(
        policy_kernel().can_parallelize_managed_offers(
            signer_path_configured,
            parallelism_enabled,
            runtime_dry_run,
            has_coordinator,
        )
    )


def parallel_max_workers(*, submission_count: int, configured_max: int) -> int:
    return int(policy_kernel().parallel_max_workers(int(submission_count), int(configured_max)))


def reservation_release_status(*, is_executed: bool) -> str:
    return str(policy_kernel().reservation_release_status(bool(is_executed)))


def should_apply_parallel_transient_cooldown(
    *,
    transient_failures: int,
    total_parallel: int,
    cooldown_seconds: int,
) -> bool:
    return bool(
        policy_kernel().should_apply_parallel_transient_cooldown(
            int(transient_failures),
            int(total_parallel),
            int(cooldown_seconds),
        )
    )


def managed_retry_decision(
    *,
    attempt_index: int,
    attempts_max: int,
    backoff_ms: int,
    is_upstream_transient: bool,
) -> ManagedRetryDecision:
    signer = policy_kernel()
    result = signer.managed_retry_decision(
        int(attempt_index),
        int(attempts_max),
        int(backoff_ms),
        bool(is_upstream_transient),
    )
    if not isinstance(result, ManagedRetryDecision):
        raise TypeError("managed_retry_decision returned non-ManagedRetryDecision result")
    return result


def classify_managed_post_result(
    *,
    success: bool,
    error_text: str,
    offer_id: str,
    publish_venue: str,
) -> ManagedActionOutcome:
    signer = policy_kernel()
    result = signer.classify_managed_post_result(success, error_text, offer_id, publish_venue)
    if not isinstance(result, ManagedActionOutcome):
        raise TypeError("classify_managed_post_result returned non-ManagedActionOutcome result")
    return result


def classify_dexie_visibility_outcome(
    *,
    visible: bool,
    visibility_error: str,
) -> ManagedActionOutcome:
    signer = policy_kernel()
    result = signer.classify_dexie_visibility_outcome(visible, visibility_error)
    if not isinstance(result, ManagedActionOutcome):
        raise TypeError(
            "classify_dexie_visibility_outcome returned non-ManagedActionOutcome result"
        )
    return result


def count_parallel_transient_failures(items: list[StrategyActionItem]) -> int:
    for index, item in enumerate(items):
        if not isinstance(item, StrategyActionItem):
            raise TypeError(
                f"parallel outcome list item {index} must be StrategyActionItem, "
                f"got {type(item).__name__}"
            )
    return int(policy_kernel().count_parallel_transient_failures(items))
