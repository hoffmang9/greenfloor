from __future__ import annotations

import os

import pytest


def _require_native_integration_enabled() -> None:
    if os.getenv("GREENFLOOR_RUN_NATIVE_INTEGRATION_TESTS", "").strip() != "1":
        pytest.skip("set GREENFLOOR_RUN_NATIVE_INTEGRATION_TESTS=1 to run greenfloor-native tests")


def _require_importable_modules():
    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception:
        pytest.skip("chia_wallet_sdk import unavailable")
    try:
        import greenfloor_native as native  # type: ignore
    except Exception:
        pytest.skip("greenfloor_native import unavailable")
    return sdk, native


def test_greenfloor_native_validate_offer_rejects_garbage() -> None:
    _require_native_integration_enabled()
    _sdk, native = _require_importable_modules()
    with pytest.raises(ValueError):
        native.validate_offer("not-an-offer")


def test_greenfloor_native_from_input_spend_bundle_xch_round_trip_offer() -> None:
    _require_native_integration_enabled()
    sdk, native = _require_importable_modules()

    # Minimal synthesized offer request: empty maker input bundle + one XCH request leg.
    input_spend_bundle = sdk.SpendBundle([], sdk.Signature.infinity())
    offer_spend_bundle_bytes = native.from_input_spend_bundle_xch(
        input_spend_bundle.to_bytes(),
        [(bytes([3]) * 32, [(bytes([4]) * 32, 42)])],
    )
    offer_spend_bundle = sdk.SpendBundle.from_bytes(offer_spend_bundle_bytes)
    offer_text = sdk.encode_offer(offer_spend_bundle)

    native.validate_offer(offer_text)
