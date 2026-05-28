"""Golden signer fixtures vs core create-offer request builder."""

from __future__ import annotations

import json
from dataclasses import replace
from pathlib import Path

import pytest

from greenfloor.core.signer_offer_request import build_signer_create_offer_request
from tests.helpers.config_fixtures import minimal_market_config

FIXTURE_DIR = Path(__file__).resolve().parent / "fixtures" / "signer"

_RUNTIME_REQUEST_FIELDS = (
    "offer_asset_id",
    "request_asset_id",
    "offer_amount",
    "request_amount",
    "split_input_coins",
    "broadcast_split",
    "expires_at",
)


def _comparable_request_fields(request: dict) -> dict:
    normalized: dict = {}
    for key in _RUNTIME_REQUEST_FIELDS:
        value = request[key]
        if key.endswith("_asset_id"):
            normalized[key] = str(value).strip().lower().removeprefix("0x")
        else:
            normalized[key] = value
    return normalized


def _market_from_parity(parity: dict) -> object:
    return replace(
        minimal_market_config(),
        receive_address="txch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqstg4h8",
        pricing={
            "base_unit_mojo_multiplier": int(parity["base_unit_mojo_multiplier"]),
            "quote_unit_mojo_multiplier": int(parity["quote_unit_mojo_multiplier"]),
        },
    )


@pytest.mark.parametrize("fixture_path", sorted(FIXTURE_DIR.glob("*.json")))
def test_fixture_runtime_parity_matches_core_builder(fixture_path: Path) -> None:
    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    parity = payload["runtime_parity"]
    fixture_req = payload["create_offer_request"]
    assert str(payload["offer"]).startswith("offer1")

    runtime_req = build_signer_create_offer_request(
        market=_market_from_parity(parity),
        size_base_units=int(parity["size_base_units"]),
        quote_price=float(parity["quote_price"]),
        resolved_base_asset_id=str(parity["resolved_base_asset_id"]),
        resolved_quote_asset_id=str(parity["resolved_quote_asset_id"]),
        action_side=str(parity["action_side"]),
        split_input_coins=bool(fixture_req["split_input_coins"]),
        broadcast_split=bool(fixture_req["broadcast_split"]),
        expires_at_unix=fixture_req.get("expires_at"),
    )

    assert _comparable_request_fields(runtime_req) == _comparable_request_fields(fixture_req)


@pytest.mark.parametrize("fixture_path", sorted(FIXTURE_DIR.glob("*.json")))
def test_signer_golden_offer_validates(fixture_path: Path) -> None:
    try:
        import greenfloor_signer  # type: ignore[import-not-found]
    except ImportError:
        pytest.skip("greenfloor_signer not installed")

    validate = getattr(greenfloor_signer, "validate_offer_structure", None)
    if not callable(validate):
        pytest.skip("greenfloor_signer.validate_offer_structure not available")

    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    offer = str(payload.get("offer", "")).strip()
    assert offer.startswith("offer1")
    assert "create_offer_request" in payload
    assert "runtime_parity" in payload
    validate(offer)


@pytest.mark.parametrize("fixture_path", sorted(FIXTURE_DIR.glob("*.json")))
def test_create_offer_request_fixture_has_required_fields(fixture_path: Path) -> None:
    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    request = payload["create_offer_request"]
    parity = payload["runtime_parity"]
    for key in (
        "receive_address",
        "offer_asset_id",
        "offer_amount",
        "request_asset_id",
        "request_amount",
        "offer_coin_ids",
        "presplit_coin_ids",
        "split_input_coins",
        "broadcast_split",
    ):
        assert key in request
    for key in (
        "action_side",
        "resolved_base_asset_id",
        "resolved_quote_asset_id",
        "size_base_units",
        "quote_price",
        "base_unit_mojo_multiplier",
        "quote_unit_mojo_multiplier",
    ):
        assert key in parity
