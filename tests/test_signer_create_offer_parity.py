"""Golden signer fixtures vs core create-offer request builder."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from greenfloor.core.signer_offer_request import build_signer_create_offer_request
from tests.helpers.signer_fixtures import (
    SIGNER_FIXTURE_DIR,
    comparable_fixture_runtime_fields,
    comparable_runtime_payload,
    market_config_from_fixture,
    parse_create_offer_request,
    parse_runtime_parity,
)


@pytest.mark.parametrize("fixture_path", sorted(SIGNER_FIXTURE_DIR.glob("*.json")))
def test_signer_golden_fixture_contract(fixture_path: Path) -> None:
    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    fixture_req, expires_at_unix = parse_create_offer_request(payload["create_offer_request"])
    parity = parse_runtime_parity(payload["runtime_parity"])

    assert str(payload["offer"]).startswith("offer1")

    runtime_req = build_signer_create_offer_request(
        market=market_config_from_fixture(create_offer_request=fixture_req, parity=parity),
        size_base_units=parity["size_base_units"],
        quote_price=parity["quote_price"],
        resolved_base_asset_id=parity["resolved_base_asset_id"],
        resolved_quote_asset_id=parity["resolved_quote_asset_id"],
        action_side=parity["action_side"],
        split_input_coins=fixture_req["split_input_coins"],
        broadcast_split=fixture_req["broadcast_split"],
        expires_at_unix=expires_at_unix,
    )
    assert comparable_runtime_payload(
        runtime_req.to_payload()
    ) == comparable_fixture_runtime_fields(
        fixture_req,
        expires_at_unix=expires_at_unix,
    )


@pytest.mark.signer
@pytest.mark.parametrize("fixture_path", sorted(SIGNER_FIXTURE_DIR.glob("*.json")))
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
    parse_create_offer_request(payload["create_offer_request"])
    parse_runtime_parity(payload["runtime_parity"])
    validate(offer)
