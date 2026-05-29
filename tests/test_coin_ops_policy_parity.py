"""Coin-op policy parity and FFI contract tests."""

from __future__ import annotations

import pytest

from greenfloor.config.models import MarketLadderEntry
from greenfloor.core.coin_ops import (
    coin_meets_coin_op_min_amount,
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
    effective_sell_bucket_counts_for_coin_ops,
    select_spendable_coins_for_target_amount,
)
from greenfloor.hex_utils import (
    canonical_is_xch,
    default_mojo_multiplier_for_asset,
    is_hex_id,
    normalize_hex_id,
)

_CAT_ID = "0000000000000000000000000000000000000000000000000000000000000001"
_VALID_HEX_ID = "a" * 64


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
    from greenfloor.core.engine_bridge import import_engine

    engine = import_engine()
    assert canonical_is_xch(asset_id) is expected_xch
    assert bool(engine.canonical_is_xch(asset_id)) is expected_xch
    if expected_xch:
        assert coin_op_min_amount_mojos(canonical_asset_id=asset_id) == 0
    else:
        assert coin_op_min_amount_mojos(canonical_asset_id=asset_id) == 1000


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (_VALID_HEX_ID, _VALID_HEX_ID),
        (f"0x{_VALID_HEX_ID}", _VALID_HEX_ID),
        ("abc", ""),
        ("g" * 64, ""),
    ],
)
def test_normalize_hex_id_parity_with_engine(value: str, expected: str) -> None:
    from greenfloor.core.engine_bridge import import_engine

    engine = import_engine()
    assert normalize_hex_id(value) == expected
    assert str(engine.normalize_hex_id(value)) == expected
    assert is_hex_id(value) is bool(expected)
    assert bool(engine.is_hex_id(value)) is bool(expected)


@pytest.mark.parametrize(
    ("asset_id", "expected"),
    [
        ("xch", 1_000_000_000_000),
        (_CAT_ID, 1_000),
    ],
)
def test_default_mojo_multiplier_parity_with_engine(asset_id: str, expected: int) -> None:
    from greenfloor.core.engine_bridge import import_engine

    engine = import_engine()
    assert default_mojo_multiplier_for_asset(asset_id) == expected
    assert int(engine.default_mojo_multiplier_for_asset(asset_id)) == expected


def test_coin_meets_min_amount_rejects_invalid_amount_type() -> None:
    assert not coin_meets_coin_op_min_amount({"amount": "not-an-int"}, canonical_asset_id=_CAT_ID)


def test_coin_meets_min_amount_treats_missing_amount_as_zero() -> None:
    assert coin_meets_coin_op_min_amount({}, canonical_asset_id="xch")
    assert not coin_meets_coin_op_min_amount({}, canonical_asset_id=_CAT_ID)


def test_spendable_coin_parse_skips_invalid_amount_rows() -> None:
    coins = [
        {"id": "valid", "amount": 5000},
        {"id": "bad_amount", "amount": "not-int"},
        {"id": "", "amount": 1000},
    ]
    coin_ids, total, exact = select_spendable_coins_for_target_amount(
        coins=coins,
        target_amount=5000,
    )
    assert coin_ids == ["valid"]
    assert total == 5000
    assert exact is True


def test_target_amount_allowed_matches_coin_meets_for_same_amount() -> None:
    amount = 1500
    assert coin_op_target_amount_allowed(amount_mojos=amount, canonical_asset_id=_CAT_ID)
    assert coin_meets_coin_op_min_amount({"amount": amount}, canonical_asset_id=_CAT_ID)
    assert not coin_op_target_amount_allowed(amount_mojos=500, canonical_asset_id=_CAT_ID)
    assert not coin_meets_coin_op_min_amount({"amount": 500}, canonical_asset_id=_CAT_ID)


def _sell_ladder(*entries: tuple[int, int]) -> list[MarketLadderEntry]:
    return [
        MarketLadderEntry(
            size_base_units=size,
            target_count=target,
            split_buffer_count=1,
            combine_when_excess_factor=2.0,
        )
        for size, target in entries
    ]


def test_effective_sell_bucket_counts_for_coin_ops_counts_live_sells_toward_target_only() -> None:
    effective = effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=_sell_ladder((10, 3)),
        wallet_bucket_counts={10: 0},
        active_sell_offer_counts_by_size={10: 3},
    )
    assert effective[10] == 3


def test_effective_sell_bucket_counts_for_coin_ops_caps_live_sell_credit_at_target() -> None:
    effective = effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=_sell_ladder((10, 3)),
        wallet_bucket_counts={10: 0},
        active_sell_offer_counts_by_size={10: 4},
    )
    assert effective[10] == 3


def test_effective_sell_bucket_counts_for_coin_ops_accounts_for_new_sell_posts_in_cycle() -> None:
    effective = effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=_sell_ladder((10, 2)),
        wallet_bucket_counts={10: 2},
        active_sell_offer_counts_by_size={10: 0},
        newly_executed_sell_offer_counts_by_size={10: 2},
    )
    assert effective[10] == 2
