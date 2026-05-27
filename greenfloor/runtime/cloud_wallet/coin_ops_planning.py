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

from dataclasses import dataclass
from enum import StrEnum
from typing import Any

from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos
from greenfloor.runtime.cloud_wallet.coin_ops_selection import (
    select_exact_amount_coin_ids,
    select_largest_spendable_coin,
    split_would_create_sub_cat_change,
)


def select_spendable_coins_for_target_amount(
    *,
    coins: list[dict[str, Any]],
    target_amount: int,
) -> tuple[list[str], int, bool]:
    """Pick spendable input coins to reach target; prefer exact sum first."""
    required = int(target_amount)
    if required <= 0:
        return [], 0, False
    entries: list[tuple[str, int]] = []
    for coin in coins:
        if not isinstance(coin, dict):
            continue
        coin_id = str(coin.get("id", "")).strip()
        if not coin_id:
            continue
        try:
            amount = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            amount = 0
        if amount <= 0:
            continue
        entries.append((coin_id, amount))
    if not entries:
        return [], 0, False

    max_amount = max(amount for _, amount in entries)
    cap = required + max_amount
    if cap > 500_000:
        ordered = sorted(entries, key=lambda row: row[1], reverse=True)
        picked_ids: list[str] = []
        running = 0
        for coin_id, amount in ordered:
            picked_ids.append(coin_id)
            running += amount
            if running >= required:
                return picked_ids, running, running == required
        return [], 0, False

    best: dict[int, list[int]] = {0: []}
    for idx, (_coin_id, amount) in enumerate(entries):
        snapshot = list(best.items())
        for prev_sum, subset in snapshot:
            next_sum = int(prev_sum) + int(amount)
            if next_sum > cap:
                continue
            candidate = subset + [idx]
            existing = best.get(next_sum)
            if existing is None or len(candidate) < len(existing):
                best[next_sum] = candidate

    exact_subset = best.get(required)
    if exact_subset is not None and len(exact_subset) > 0:
        ids = [entries[i][0] for i in exact_subset]
        total = sum(entries[i][1] for i in exact_subset)
        return ids, total, True

    overs = [s for s in best.keys() if s > required]
    if not overs:
        return [], 0, False
    best_over = min(
        overs,
        key=lambda s: (
            int(s) - required,
            len(best.get(s, [])),
            int(s),
        ),
    )
    subset = best.get(best_over, [])
    if not subset:
        return [], 0, False
    ids = [entries[i][0] for i in subset]
    total = sum(entries[i][1] for i in subset)
    return ids, total, False


class CombineInputSelectionMode(StrEnum):
    LARGEST_BY_AMOUNT = "largest_by_amount"
    EXACT_AMOUNT = "exact_amount"


class SplitPlanningProfile(StrEnum):
    """Named split auto-select behavior for CLI vs daemon."""

    CLI_AUTO = "cli_auto"
    DAEMON_AUTO = "daemon_auto"


@dataclass(frozen=True, slots=True)
class _SplitPlanningBehavior:
    enforce_required_amount: bool
    check_sub_cat_change: bool
    default_allow_combine_prereq: bool


_SPLIT_PLANNING_BEHAVIOR: dict[SplitPlanningProfile, _SplitPlanningBehavior] = {
    SplitPlanningProfile.CLI_AUTO: _SplitPlanningBehavior(
        enforce_required_amount=False,
        check_sub_cat_change=False,
        default_allow_combine_prereq=False,
    ),
    SplitPlanningProfile.DAEMON_AUTO: _SplitPlanningBehavior(
        enforce_required_amount=True,
        check_sub_cat_change=True,
        default_allow_combine_prereq=True,
    ),
}


@dataclass(frozen=True, slots=True)
class SplitCombinePrereqPlan:
    input_coin_ids: list[str]
    target_amount: int
    selected_total: int
    exact_match: bool
    cap_applied: bool
    selected_count_before_cap: int
    combine_input_cap: int


@dataclass(frozen=True, slots=True)
class SplitCoinPlan:
    coin_id: str
    selected_amount_mojos: int


@dataclass(frozen=True, slots=True)
class SplitSkipPlan:
    reason: str
    data: dict[str, Any] | None = None


SplitAutoSelectPlan = SplitCoinPlan | SplitCombinePrereqPlan | SplitSkipPlan


def build_combine_prereq_plan(
    *,
    candidate_spendable: list[dict],
    required_amount_mojos: int,
    combine_input_cap: int,
) -> SplitCombinePrereqPlan | None:
    combine_coin_ids, _combine_total, _exact_match = select_spendable_coins_for_target_amount(
        coins=candidate_spendable,
        target_amount=required_amount_mojos,
    )
    if len(combine_coin_ids) < 2:
        return None
    amount_by_coin_id = {
        str(coin.get("id", "")).strip(): int(coin.get("amount", 0)) for coin in candidate_spendable
    }
    combine_input_coin_ids = list(combine_coin_ids[:combine_input_cap])
    combine_cap_applied = len(combine_input_coin_ids) < len(combine_coin_ids)
    combine_selected_total = sum(
        amount_by_coin_id.get(coin_id, 0) for coin_id in combine_input_coin_ids
    )
    combine_exact_match = combine_selected_total == required_amount_mojos
    combine_target_amount = (
        required_amount_mojos
        if combine_selected_total >= required_amount_mojos
        else combine_selected_total
    )
    return SplitCombinePrereqPlan(
        input_coin_ids=combine_input_coin_ids,
        target_amount=combine_target_amount,
        selected_total=combine_selected_total,
        exact_match=combine_exact_match,
        cap_applied=combine_cap_applied,
        selected_count_before_cap=len(combine_coin_ids),
        combine_input_cap=combine_input_cap,
    )


def plan_auto_split_selection(
    *,
    candidate_spendable: list[dict],
    required_amount_mojos: int,
    canonical_asset_id: str,
    profile: SplitPlanningProfile,
    combine_input_cap: int,
    allow_combine_prereq: bool | None = None,
) -> SplitAutoSelectPlan:
    """Plan the next auto-selected split action from filtered spendable coins."""
    behavior = _SPLIT_PLANNING_BEHAVIOR[profile]
    resolve_allow_combine_prereq = (
        allow_combine_prereq
        if allow_combine_prereq is not None
        else behavior.default_allow_combine_prereq
    )
    enforce_required_amount = behavior.enforce_required_amount
    check_sub_cat_change = behavior.check_sub_cat_change

    if enforce_required_amount and required_amount_mojos > 0:
        large_enough = [
            coin
            for coin in candidate_spendable
            if int(coin.get("amount", 0)) >= required_amount_mojos
        ]
    else:
        large_enough = list(candidate_spendable)

    if large_enough:
        selected_coin = select_largest_spendable_coin(
            large_enough,
            min_amount_mojos=required_amount_mojos if enforce_required_amount else 0,
        )
        if selected_coin is None:
            return SplitSkipPlan(reason="no_spendable_split_coin_meets_required_amount")
        selected_coin_id = str(selected_coin.get("id", "")).strip()
        if not selected_coin_id:
            return SplitSkipPlan(reason="no_spendable_split_coin_meets_required_amount")
        selected_amount = int(selected_coin.get("amount", 0))
        if check_sub_cat_change and enforce_required_amount:
            would_create_dust, remainder = split_would_create_sub_cat_change(
                selected_amount_mojos=selected_amount,
                required_amount_mojos=required_amount_mojos,
                canonical_asset_id=canonical_asset_id,
            )
            if would_create_dust:
                return SplitSkipPlan(
                    reason="split_would_create_sub_cat_change",
                    data={
                        "selected_coin_id": selected_coin_id,
                        "selected_amount_mojos": int(selected_amount),
                        "required_amount_mojos": int(required_amount_mojos),
                        "remainder_mojos": int(remainder),
                        "minimum_allowed_mojos": int(
                            coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
                        ),
                    },
                )
        return SplitCoinPlan(coin_id=selected_coin_id, selected_amount_mojos=selected_amount)

    if (
        resolve_allow_combine_prereq
        and enforce_required_amount
        and required_amount_mojos > 0
        and sum(int(coin.get("amount", 0)) for coin in candidate_spendable) >= required_amount_mojos
    ):
        prereq = build_combine_prereq_plan(
            candidate_spendable=candidate_spendable,
            required_amount_mojos=required_amount_mojos,
            combine_input_cap=combine_input_cap,
        )
        if prereq is not None:
            return prereq

    return SplitSkipPlan(reason="no_spendable_split_coin_meets_required_amount")


def plan_auto_combine_inputs(
    *,
    spendable_coins: list[dict],
    number_of_coins: int,
    selection_mode: CombineInputSelectionMode,
    target_amount_mojos: int | None = None,
    exclude_coin_ids: set[str] | None = None,
    max_count: int | None = None,
) -> list[str]:
    capped_count = (
        min(int(number_of_coins), int(max_count)) if max_count is not None else int(number_of_coins)
    )
    if selection_mode == CombineInputSelectionMode.EXACT_AMOUNT:
        if target_amount_mojos is None:
            raise ValueError("target_amount_mojos is required for exact-amount combine selection")
        return select_exact_amount_coin_ids(
            spendable_coins,
            amount_mojos=int(target_amount_mojos),
            exclude_coin_ids=exclude_coin_ids,
            max_count=capped_count,
        )

    excluded = {value.lower() for value in (exclude_coin_ids or set())}
    eligible = [
        coin
        for coin in spendable_coins
        if isinstance(coin, dict)
        and str(coin.get("id", "")).strip()
        and str(coin.get("id", "")).strip().lower() not in excluded
    ]
    eligible.sort(key=lambda coin: int(coin.get("amount", 0)), reverse=True)
    return [str(coin.get("id", "")).strip() for coin in eligible[:capped_count]]
