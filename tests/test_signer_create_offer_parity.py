"""Golden signer fixtures vs core create-offer request builder."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from greenfloor.core.signer_offer_request import (
    SignerOfferLegAmounts,
    build_signer_create_offer_request,
    compute_signer_offer_leg_amounts,
    quote_mojos_for_base_size,
    signer_split_asset_id,
)
from tests.helpers.signer_fixtures import (
    SIGNER_FIXTURE_DIR,
    comparable_fixture_runtime_fields,
    comparable_runtime_payload,
    market_config_from_fixture,
    parse_create_offer_request,
    parse_runtime_parity,
)


def _require_signer_kernel() -> None:
    try:
        import greenfloor_signer  # type: ignore[import-not-found]  # noqa: F401
    except ImportError:
        pytest.skip("greenfloor_signer not installed")
    compute = getattr(greenfloor_signer, "compute_signer_offer_leg_amounts", None)
    if not callable(compute):
        pytest.skip("greenfloor_signer.compute_signer_offer_leg_amounts not available")


@pytest.mark.parametrize("fixture_path", sorted(SIGNER_FIXTURE_DIR.glob("*.json")))
def test_signer_golden_fixture_contract(fixture_path: Path) -> None:
    _require_signer_kernel()
    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    fixture_req, expires_at_unix = parse_create_offer_request(payload["create_offer_request"])
    parity = parse_runtime_parity(payload["runtime_parity"])

    assert str(payload["offer"]).startswith("offer1")

    market = market_config_from_fixture(create_offer_request=fixture_req, parity=parity)
    leg = compute_signer_offer_leg_amounts(
        size_base_units=parity["size_base_units"],
        quote_price=parity["quote_price"],
        resolved_base_asset_id=parity["resolved_base_asset_id"],
        resolved_quote_asset_id=parity["resolved_quote_asset_id"],
        action_side=parity["action_side"],
        pricing=dict(market.pricing or {}),
    )
    assert isinstance(leg, SignerOfferLegAmounts)
    assert leg.offer_amount_mojos == int(fixture_req["offer_amount"])
    assert leg.request_amount_mojos == int(fixture_req["request_amount"])

    runtime_req = build_signer_create_offer_request(
        market=market,
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


def test_quote_mojos_for_base_size_matches_direct_fixture_pricing() -> None:
    _require_signer_kernel()
    assert quote_mojos_for_base_size(
        size_base_units=1,
        quote_price=1.0,
        quote_unit_multiplier=1_000_000_000_000,
    ) == 1_000_000_000_000


def test_signer_split_asset_id_selects_side_asset() -> None:
    _require_signer_kernel()
    base = "457275a8b9926058d8c9c2ebae52ac5938a4034345cafef6e87f4c7c24b749d8"
    quote = "664799fc173e0d9d4d024c42e411d26f275eeb1095dad980ccd11df09c8bb6fb"
    assert (
        signer_split_asset_id(
            action_side="sell",
            resolved_base_asset_id=base,
            resolved_quote_asset_id=quote,
        )
        == base
    )
    assert (
        signer_split_asset_id(
            action_side="buy",
            resolved_base_asset_id=base,
            resolved_quote_asset_id=quote,
        )
        == quote
    )


@pytest.mark.parametrize(
    ("size_base_units", "quote_price", "match"),
    [
        (1, 0.0, "request_amount must be positive"),
        (0, 1.0, "invalid_size_base_units"),
    ],
)
def test_compute_signer_offer_leg_amounts_rejects_invalid_inputs(
    size_base_units: int,
    quote_price: float,
    match: str,
) -> None:
    _require_signer_kernel()
    with pytest.raises(ValueError, match=match):
        compute_signer_offer_leg_amounts(
            size_base_units=size_base_units,
            quote_price=quote_price,
            resolved_base_asset_id="457275a8b9926058d8c9c2ebae52ac5938a4034345cafef6e87f4c7c24b749d8",
            resolved_quote_asset_id="xch",
            action_side="sell",
            pricing={
                "base_unit_mojo_multiplier": 1000,
                "quote_unit_mojo_multiplier": 1000,
            },
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
