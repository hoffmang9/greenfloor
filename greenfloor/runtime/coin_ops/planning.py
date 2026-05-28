"""Shared coin split/combine planning used by CLI steps and daemon execution.

``SplitPlanningProfile`` controls auto-select behavior in
``plan_auto_split_selection()``:

+---------------+-------------------------------+--------------------+------------------------------------------------------------------------------+
| Profile       | Required amount               | Sub-CAT dust guard | Combine-for-split prereq                                                     |
+===============+===============================+====================+==============================================================================+
| ``CLI_AUTO``  | off (largest min-amount coin) | off                | off                                                                          |
+---------------+-------------------------------+--------------------+------------------------------------------------------------------------------+
| ``DAEMON_AUTO`` | on                          | on                 | on (first attempt only; caller passes ``allow_combine_prereq=False`` on retry) |
+---------------+-------------------------------+--------------------+------------------------------------------------------------------------------+

CLI split is operator-driven: auto-select picks the largest spendable coin meeting
min amount; the operator (or explicit ``--coin-id``) owns total-value checks.
Daemon split is ladder-driven and must enforce total required value, avoid
sub-CAT change dust, and may submit a combine-for-split prereq on the first
attempt only.
"""

from __future__ import annotations

from greenfloor.core.coin_ops import (
    CombineInputSelectionMode,
    SplitAutoSelectPlan,
    SplitCoinPlan,
    SplitCombinePrereqPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
    plan_auto_combine_inputs as _plan_auto_combine_inputs,
    plan_auto_split_selection as _plan_auto_split_selection,
    select_spendable_coins_for_target_amount,
)

__all__ = [
    "CombineInputSelectionMode",
    "SplitAutoSelectPlan",
    "SplitCoinPlan",
    "SplitCombinePrereqPlan",
    "SplitPlanningProfile",
    "SplitSkipPlan",
    "plan_auto_combine_inputs",
    "plan_auto_split_selection",
    "select_spendable_coins_for_target_amount",
]


def plan_auto_split_selection(
    *,
    candidate_spendable: list[dict],
    required_amount_mojos: int,
    canonical_asset_id: str,
    profile: SplitPlanningProfile,
    combine_input_cap: int,
    allow_combine_prereq: bool | None = None,
) -> SplitAutoSelectPlan:
    return _plan_auto_split_selection(
        candidate_spendable=candidate_spendable,
        required_amount_mojos=required_amount_mojos,
        canonical_asset_id=canonical_asset_id,
        profile=profile.value,
        combine_input_cap=combine_input_cap,
        allow_combine_prereq=allow_combine_prereq,
    )


def plan_auto_combine_inputs(
    *,
    spendable_coins: list[dict],
    number_of_coins: int,
    selection_mode: CombineInputSelectionMode,
    target_amount_mojos: int | None = None,
    exclude_coin_ids: set[str] | None = None,
    max_count: int | None = None,
) -> list[str]:
    return _plan_auto_combine_inputs(
        spendable_coins=spendable_coins,
        number_of_coins=number_of_coins,
        selection_mode=selection_mode.value,
        target_amount_mojos=target_amount_mojos,
        exclude_coin_ids=exclude_coin_ids,
        max_count=max_count,
    )
