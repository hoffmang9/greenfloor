"""Coin-op policy parity and FFI contract tests."""

from __future__ import annotations

import pytest

from greenfloor.core.coin_ops import (
    coin_meets_coin_op_min_amount,
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
)
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


def test_target_amount_allowed_matches_coin_meets_for_same_amount() -> None:
    amount = 1500
    assert coin_op_target_amount_allowed(amount_mojos=amount, canonical_asset_id=_CAT_ID)
    assert coin_meets_coin_op_min_amount({"amount": amount}, canonical_asset_id=_CAT_ID)
    assert not coin_op_target_amount_allowed(amount_mojos=500, canonical_asset_id=_CAT_ID)
    assert not coin_meets_coin_op_min_amount({"amount": 500}, canonical_asset_id=_CAT_ID)
