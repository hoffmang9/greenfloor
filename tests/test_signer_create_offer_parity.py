"""Golden signer fixtures vs core create-offer request builder."""

from __future__ import annotations

import json
from collections.abc import Callable
from dataclasses import replace
from pathlib import Path
from typing import cast

import pytest

from greenfloor.core.offer_policy import normalize_offer_side
from greenfloor.core.planned_action import PlannedAction, planned_action_side
from greenfloor.core.signer_offer_request import (
    build_signer_create_offer_request,
    compute_signer_offer_leg_amounts,
)
from tests.helpers.offer_runtime_fixtures import (
    market_config_for_local_offer,
    offer_build_context_for_program_market,
    program_config_for_local_offer,
)
from tests.helpers.signer_fixtures import (
    SIGNER_FIXTURE_DIR,
    comparable_fixture_runtime_fields,
    comparable_runtime_payload,
    market_config_from_fixture,
    parse_create_offer_request,
    parse_runtime_parity,
)


def _require_signer_kernel():
    try:
        import greenfloor_signer as kernel  # type: ignore[import-not-found]
    except ImportError:
        pytest.skip("greenfloor_signer not installed")
    if not callable(getattr(kernel, "compute_signer_offer_leg_amounts", None)):
        pytest.skip("greenfloor_signer.compute_signer_offer_leg_amounts not available")
    if not callable(getattr(kernel, "normalize_offer_side", None)):
        pytest.skip("greenfloor_signer.normalize_offer_side not available")
    return kernel


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("buy", "buy"),
        ("BUY", "buy"),
        ("sell", "sell"),
        ("", "sell"),
    ],
)
def test_normalize_offer_side_matches_kernel(raw: str, expected: str) -> None:
    kernel = _require_signer_kernel()
    assert normalize_offer_side(raw) == expected
    kernel_normalize = cast(
        Callable[[str], str],
        kernel.normalize_offer_side,  # pyright: ignore[reportAttributeAccessIssue]
    )
    assert str(kernel_normalize(str(raw))) == expected


def test_planned_action_side_avoids_kernel_for_canonical_labels() -> None:
    action = PlannedAction(
        size=1,
        repeat=1,
        pair="xch",
        expiry_unit="minutes",
        expiry_value=10,
        cancel_after_create=False,
        reason="test",
        side="buy",
    )
    assert planned_action_side(action) == "buy"


def test_prepare_offer_build_context_caches_normalized_side(tmp_path: Path) -> None:
    program = program_config_for_local_offer(home_dir=str(tmp_path))
    market = replace(
        market_config_for_local_offer(),
        pricing={"fixed_quote_per_base": 1.0},
    )
    program_path = tmp_path / "program.yaml"
    program_path.write_text("app:\n  network: mainnet\n", encoding="utf-8")
    build_ctx = offer_build_context_for_program_market(
        program=program,
        market=market,
        program_path=program_path,
        action_side="BUY",
    )
    assert build_ctx.action_side == "buy"


@pytest.mark.parametrize("fixture_path", sorted(SIGNER_FIXTURE_DIR.glob("*.json")))
def test_signer_golden_fixture_contract(fixture_path: Path) -> None:
    _require_signer_kernel()
    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    fixture_req, expires_at_unix = parse_create_offer_request(payload["create_offer_request"])
    parity = parse_runtime_parity(payload["runtime_parity"])

    assert str(payload["offer"]).startswith("offer1")

    market = market_config_from_fixture(create_offer_request=fixture_req, parity=parity)
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
    runtime_payload = runtime_req.to_payload()
    assert int(runtime_payload["offer_amount"]) == int(fixture_req["offer_amount"])
    assert int(runtime_payload["request_amount"]) == int(fixture_req["request_amount"])
    assert comparable_runtime_payload(runtime_payload) == comparable_fixture_runtime_fields(
        fixture_req,
        expires_at_unix=expires_at_unix,
    )


@pytest.mark.parametrize(
    ("size_base_units", "quote_price", "match"),
    [
        (1, 0.0, "request_amount must be positive"),
        (0, 1.0, "invalid_size_base_units"),
        (1, 1.0, "invalid_offer_amount"),
    ],
)
def test_compute_signer_offer_leg_amounts_rejects_invalid_inputs(
    size_base_units: int,
    quote_price: float,
    match: str,
) -> None:
    _require_signer_kernel()
    pricing = {
        "base_unit_mojo_multiplier": 1000,
        "quote_unit_mojo_multiplier": 1000,
    }
    if match == "invalid_offer_amount":
        pricing["base_unit_mojo_multiplier"] = 0
    with pytest.raises(ValueError, match=match):
        compute_signer_offer_leg_amounts(
            size_base_units=size_base_units,
            quote_price=quote_price,
            resolved_base_asset_id="457275a8b9926058d8c9c2ebae52ac5938a4034345cafef6e87f4c7c24b749d8",
            resolved_quote_asset_id="xch",
            action_side="sell",
            pricing=pricing,
        )


@pytest.mark.signer
@pytest.mark.parametrize("fixture_path", sorted(SIGNER_FIXTURE_DIR.glob("*.json")))
def test_signer_golden_offer_validates(fixture_path: Path) -> None:
    kernel = _require_signer_kernel()
    validate = getattr(kernel, "validate_offer_structure", None)
    if not callable(validate):
        pytest.skip("greenfloor_signer.validate_offer_structure not available")

    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    offer = str(payload.get("offer", "")).strip()
    assert offer.startswith("offer1")
    parse_create_offer_request(payload["create_offer_request"])
    parse_runtime_parity(payload["runtime_parity"])
    validate(offer)
