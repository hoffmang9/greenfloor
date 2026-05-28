"""Pure planning for parallel managed-offer submission (no IO)."""

from __future__ import annotations

from dataclasses import dataclass

from greenfloor.core.cycle import plan_parallel_submission_batch
from greenfloor.core.parallel_batch_plan import ParallelBatchPlan
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.strategy_dispatch.items import managed_skip_item


@dataclass(frozen=True, slots=True)
class ParallelSubmissionPlanEntry:
    submit_index: int
    requested_amounts: dict[str, int]


@dataclass(frozen=True, slots=True)
class PlannedParallelSubmission:
    submit_index: int
    action: PlannedAction
    requested_amounts: dict[str, int]
    available_amounts: dict[str, int]


@dataclass(frozen=True, slots=True)
class ParallelDispatchPlan:
    skip_items: list[StrategyActionItem]
    submissions: list[PlannedParallelSubmission]


def build_parallel_dispatch_plan(
    *,
    expanded_actions: list[PlannedAction],
    entries: list[ParallelSubmissionPlanEntry],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> ParallelDispatchPlan:
    batch_entries = [
        {
            "submit_index": entry.submit_index,
            "requested_amounts": entry.requested_amounts,
            "spendable_profiles": spendable_profiles,
        }
        for entry in entries
    ]
    plan: ParallelBatchPlan = plan_parallel_submission_batch(batch_entries)
    skip_items: list[StrategyActionItem] = []
    for skip in plan.skip_items:
        skip_items.append(
            managed_skip_item(
                action=expanded_actions[skip.submit_index],
                reason=skip.reason,
            )
        )
    submissions: list[PlannedParallelSubmission] = []
    for queue in plan.queue:
        submissions.append(
            PlannedParallelSubmission(
                submit_index=queue.submit_index,
                action=expanded_actions[queue.submit_index],
                requested_amounts=dict(queue.requested_amounts),
                available_amounts=dict(queue.available_amounts),
            )
        )
    return ParallelDispatchPlan(skip_items=skip_items, submissions=submissions)
