from __future__ import annotations

import os

import pytest


def _require_signer_integration_enabled() -> None:
    if os.getenv("GREENFLOOR_RUN_SIGNER_INTEGRATION_TESTS", "").strip() != "1":
        pytest.skip("set GREENFLOOR_RUN_SIGNER_INTEGRATION_TESTS=1 to run greenfloor-signer tests")


def _require_importable_modules():
    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception:
        pytest.skip("chia_wallet_sdk import unavailable")
    try:
        import greenfloor_signer as signer  # type: ignore
    except Exception:
        pytest.skip("greenfloor_signer import unavailable")
    return sdk, signer


def test_greenfloor_signer_validate_offer_rejects_garbage() -> None:
    _require_signer_integration_enabled()
    _sdk, signer = _require_importable_modules()
    with pytest.raises(ValueError):
        signer.validate_offer("not-an-offer")


def test_greenfloor_signer_from_input_spend_bundle_xch_round_trip_offer() -> None:
    _require_signer_integration_enabled()
    sdk, signer = _require_importable_modules()

    # Minimal synthesized offer request: empty maker input bundle + one XCH request leg.
    input_spend_bundle = sdk.SpendBundle([], sdk.Signature.infinity())
    offer_spend_bundle_bytes = signer.from_input_spend_bundle_xch(
        input_spend_bundle.to_bytes(),
        [(bytes([3]) * 32, [(bytes([4]) * 32, 42)])],
    )
    from greenfloor.adapters.native_offer import encode_offer_from_spend_bundle_hex

    offer_text = encode_offer_from_spend_bundle_hex(offer_spend_bundle_bytes.hex())
    assert offer_text.startswith("offer1")
    # Structural round-trip only; minimal fixture has no ASSERT_BEFORE_* expiry legs.
    signer.validate_offer_structure(offer_text)
    with pytest.raises(ValueError, match="offer_missing_expiration"):
        signer.validate_offer(offer_text)
