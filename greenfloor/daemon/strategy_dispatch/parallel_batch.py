"""Pure planning for parallel managed-offer submission (no IO)."""

from __future__ import annotations

from dataclasses import dataclass
from greenfloor.core.cycle import plan_parallel_submission_batch
from greenfloor.core.planned_action import PlannedAction
from greenfloor.daemon.strategy_dispatch.items import managed_skip_item
from greenfloor.daemon.strategy_action_item import StrategyActionItem


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
    pending_entries: list[tuple[int, PlannedAction, dict[str, int]]],
    spendable_profiles: dict[str, dict[str, int | bool]],
) -> ParallelDispatchPlan:
    batch_entries = [
        {
            "submit_index": submit_index,
            "requested_amounts": requested_amounts,
            "spendable_profiles": spendable_profiles,
        }
        for submit_index, _action, requested_amounts in pending_entries
    ]
    plan = plan_parallel_submission_batch(batch_entries)
    skip_items: list[StrategyActionItem] = []
    for skip in plan.get("skip_items", []):
        submit_index = int(skip["submit_index"])
        skip_items.append(
            managed_skip_item(
                action=expanded_actions[submit_index],
                reason=str(skip.get("reason", "skipped")),
            )
        )
    submissions: list[PlannedParallelSubmission] = []
    for queue in plan.get("queue", []):
        submit_index = int(queue["submit_index"])
        submissions.append(
            PlannedParallelSubmission(
                submit_index=submit_index,
                action=expanded_actions[submit_index],
                requested_amounts={
                    str(asset_id): int(amount)
                    for asset_id, amount in dict(queue.get("requested_amounts", {})).items()
                },
                available_amounts={
                    str(asset_id): int(amount)
                    for asset_id, amount in dict(queue.get("available_amounts", {})).items()
                },
            )
        )
    return ParallelDispatchPlan(skip_items=skip_items, submissions=submissions)
