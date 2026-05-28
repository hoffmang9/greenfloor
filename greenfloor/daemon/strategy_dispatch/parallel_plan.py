"""Map Rust parallel batch plans to dispatch submissions."""

from __future__ import annotations

from dataclasses import dataclass

from greenfloor.core.parallel_batch_plan import ParallelBatchPlan
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.strategy_action_item import StrategyActionItem
from greenfloor.daemon.strategy_dispatch.items import managed_skip_item


@dataclass(frozen=True, slots=True)
class PlannedParallelSubmission:
    submit_index: int
    action: PlannedAction
    requested_amounts: dict[str, int]
    available_amounts: dict[str, int]


def parallel_dispatch_plan_from_batch(
    *,
    expanded_actions: list[PlannedAction],
    plan: ParallelBatchPlan,
) -> tuple[list[StrategyActionItem], list[PlannedParallelSubmission]]:
    skip_items = [
        managed_skip_item(
            action=expanded_actions[skip.submit_index],
            reason=skip.reason,
        )
        for skip in plan.skip_items
    ]
    submissions = [
        PlannedParallelSubmission(
            submit_index=queue.submit_index,
            action=expanded_actions[queue.submit_index],
            requested_amounts=dict(queue.requested_amounts),
            available_amounts=dict(queue.available_amounts),
        )
        for queue in plan.queue
    ]
    return skip_items, submissions
