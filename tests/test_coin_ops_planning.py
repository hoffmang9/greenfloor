from __future__ import annotations

from greenfloor.runtime.cloud_wallet.coin_ops_planning import (
    SplitCoinPlan,
    SplitCombinePrereqPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
    plan_auto_split_selection,
)


def _coins(*rows: tuple[str, int]) -> list[dict]:
    return [{"id": coin_id, "amount": amount} for coin_id, amount in rows]


def test_cli_auto_profile_picks_largest_without_required_amount_enforcement() -> None:
    selection = plan_auto_split_selection(
        candidate_spendable=_coins(("Coin_small", 100), ("Coin_big", 1500)),
        required_amount_mojos=1000,
        canonical_asset_id="xch",
        profile=SplitPlanningProfile.CLI_AUTO,
        combine_input_cap=0,
    )
    assert isinstance(selection, SplitCoinPlan)
    assert selection.coin_id == "Coin_big"


def test_daemon_auto_profile_requires_single_coin_at_least_required_amount() -> None:
    selection = plan_auto_split_selection(
        candidate_spendable=_coins(("Coin_small", 500), ("Coin_big", 1500)),
        required_amount_mojos=1000,
        canonical_asset_id="xch",
        profile=SplitPlanningProfile.DAEMON_AUTO,
        combine_input_cap=10,
        allow_combine_prereq=False,
    )
    assert isinstance(selection, SplitCoinPlan)
    assert selection.coin_id == "Coin_big"


def test_daemon_auto_profile_skips_when_no_coin_meets_required_amount() -> None:
    selection = plan_auto_split_selection(
        candidate_spendable=_coins(("Coin_a", 400), ("Coin_b", 500)),
        required_amount_mojos=1000,
        canonical_asset_id="xch",
        profile=SplitPlanningProfile.DAEMON_AUTO,
        combine_input_cap=10,
        allow_combine_prereq=False,
    )
    assert isinstance(selection, SplitSkipPlan)
    assert selection.reason == "no_spendable_split_coin_meets_required_amount"


def test_daemon_auto_profile_returns_combine_prereq_when_aggregate_covers_required() -> None:
    selection = plan_auto_split_selection(
        candidate_spendable=_coins(("Coin_a", 4000), ("Coin_b", 6000)),
        required_amount_mojos=10_000,
        canonical_asset_id="xch",
        profile=SplitPlanningProfile.DAEMON_AUTO,
        combine_input_cap=10,
        allow_combine_prereq=True,
    )
    assert isinstance(selection, SplitCombinePrereqPlan)
    assert set(selection.input_coin_ids) == {"Coin_a", "Coin_b"}
    assert selection.exact_match is True


def test_cli_auto_profile_does_not_return_combine_prereq() -> None:
    selection = plan_auto_split_selection(
        candidate_spendable=_coins(("Coin_a", 4000), ("Coin_b", 6000)),
        required_amount_mojos=10_000,
        canonical_asset_id="xch",
        profile=SplitPlanningProfile.CLI_AUTO,
        combine_input_cap=10,
    )
    assert isinstance(selection, SplitCoinPlan)
    assert selection.coin_id == "Coin_b"


def test_daemon_auto_profile_rejects_sub_cat_change_dust() -> None:
    selection = plan_auto_split_selection(
        candidate_spendable=_coins(("Coin_cat", 10_500)),
        required_amount_mojos=10_000,
        canonical_asset_id=("0000000000000000000000000000000000000000000000000000000000000001"),
        profile=SplitPlanningProfile.DAEMON_AUTO,
        combine_input_cap=10,
        allow_combine_prereq=False,
    )
    assert isinstance(selection, SplitSkipPlan)
    assert selection.reason == "split_would_create_sub_cat_change"
    assert selection.data is not None
    assert selection.data["remainder_mojos"] == 500
