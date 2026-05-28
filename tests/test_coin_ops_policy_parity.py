"""Coin-op policy parity and FFI contract tests."""

from __future__ import annotations

import pytest

from greenfloor.core.coin_ops import BucketSpec, CoinOpPlan, plan_coin_ops
from greenfloor.core.coin_ops.policy import (
    coin_meets_coin_op_min_amount,
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
)
from greenfloor.core.kernel_bridge import import_kernel
from greenfloor.hex_utils import canonical_is_xch

_CAT_ID = "0000000000000000000000000000000000000000000000000000000000000001"


@pytest.mark.parametrize(
    ("asset_id", "expected_xch"),
    [
        ("xch", True),
        ("TXCH", True),
        ("1", True),
        ("", False),
        ("  ", False),
        (_CAT_ID, False),
    ],
)
def test_canonical_xch_parity_with_hex_utils(asset_id: str, expected_xch: bool) -> None:
    assert canonical_is_xch(asset_id) is expected_xch
    if expected_xch:
        assert coin_op_min_amount_mojos(canonical_asset_id=asset_id) == 0
    else:
        assert coin_op_min_amount_mojos(canonical_asset_id=asset_id) == 1000


def test_coin_meets_min_amount_rejects_invalid_amount_type() -> None:
    assert not coin_meets_coin_op_min_amount({"amount": "not-an-int"}, canonical_asset_id=_CAT_ID)


def test_coin_meets_min_amount_treats_missing_amount_as_zero() -> None:
    assert coin_meets_coin_op_min_amount({}, canonical_asset_id="xch")
    assert not coin_meets_coin_op_min_amount({}, canonical_asset_id=_CAT_ID)


def test_target_amount_allowed_matches_threshold_helper() -> None:
    kernel = import_kernel()
    amount = 1500
    assert coin_op_target_amount_allowed(amount_mojos=amount, canonical_asset_id=_CAT_ID)
    assert bool(kernel.coin_op_target_amount_allowed(amount, _CAT_ID))


def test_plan_coin_ops_returns_typed_plans() -> None:
    plans = plan_coin_ops(
        buckets=[
            BucketSpec(
                size_base_units=1,
                target_count=5,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
                current_count=2,
            )
        ],
        max_operations_per_run=10,
        max_fee_budget_mojos=100,
        split_fee_mojos=1,
        combine_fee_mojos=1,
    )
    assert plans
    assert isinstance(plans[0], CoinOpPlan)
    assert plans[0].op_type == "split"
