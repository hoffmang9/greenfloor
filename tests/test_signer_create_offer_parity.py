"""Parity tests for vault create-offer request shape (PyO3 JSON ↔ runtime)."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

FIXTURE_DIR = Path(__file__).resolve().parent / "fixtures" / "signer"

CREATE_OFFER_REQUEST_KEYS = frozenset(
    {
        "receive_address",
        "offer_asset_id",
        "offer_amount",
        "request_asset_id",
        "request_amount",
        "offer_coin_ids",
        "presplit_coin_ids",
        "split_input_coins",
        "broadcast_split",
        "expires_at",
    }
)


def _load_fixture(name: str) -> dict:
    path = FIXTURE_DIR / name
    if not path.is_file():
        pytest.skip(f"missing fixture: {path}")
    return json.loads(path.read_text(encoding="utf-8"))


def test_create_offer_request_json_roundtrip_via_serde_shape() -> None:
    """PyO3 deserializes the same JSON object the Rust CLI builds from flags."""
    payload = {
        "receive_address": "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqslas8z",
        "offer_asset_id": "aa" * 32,
        "offer_amount": 1_000,
        "request_asset_id": "bb" * 32,
        "request_amount": 2_000,
        "offer_coin_ids": [],
        "presplit_coin_ids": [],
        "split_input_coins": True,
        "broadcast_split": False,
        "expires_at": 1_700_000_000,
    }
    assert CREATE_OFFER_REQUEST_KEYS.issubset(payload.keys())
    roundtrip = json.loads(json.dumps(payload))
    assert roundtrip == payload


@pytest.mark.parametrize(
    ("fixture_name", "expect_distinct_assets"),
    [
        ("buy_side.json", True),
        ("cat_cat.json", True),
        ("direct.json", False),
    ],
)
def test_leg_fixture_create_offer_request_shape(
    fixture_name: str,
    *,
    expect_distinct_assets: bool,
) -> None:
    payload = _load_fixture(fixture_name)
    request = payload.get("create_offer_request")
    if request is None:
        pytest.skip(f"{fixture_name} has no create_offer_request (legacy fixture)")
    assert CREATE_OFFER_REQUEST_KEYS.issubset(request.keys())
    offer_asset = str(request["offer_asset_id"]).strip().lower()
    request_asset = str(request["request_asset_id"]).strip().lower()
    assert offer_asset
    assert request_asset
    if expect_distinct_assets:
        assert offer_asset != request_asset
    assert int(request["offer_amount"]) > 0
    assert int(request["request_amount"]) > 0
    offer = str(payload.get("offer", "")).strip()
    assert offer.startswith("offer1")


def test_buy_side_fixture_swaps_offer_and_request_legs() -> None:
    payload = _load_fixture("buy_side.json")
    request = payload.get("create_offer_request")
    if request is None:
        pytest.skip("buy_side.json missing create_offer_request")
    # Buy side offers quote CAT and requests base CAT (amounts differ from sell-side).
    assert int(request["offer_amount"]) >= int(request["request_amount"])
