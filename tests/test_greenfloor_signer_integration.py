"""Integration tests for the greenfloor_signer PyO3 extension."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

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

    input_spend_bundle = sdk.SpendBundle([], sdk.Signature.infinity())
    offer_spend_bundle_bytes = signer.from_input_spend_bundle_xch(
        input_spend_bundle.to_bytes(),
        [(bytes([3]) * 32, [(bytes([4]) * 32, 42)])],
    )
    offer_text = signer.encode_offer(offer_spend_bundle_bytes)
    assert str(offer_text).startswith("offer1")
    # Synthetic round-trip offers have no expiry; structure validation is the contract here.
    signer.validate_offer_structure(offer_text)


def test_greenfloor_signer_config_yaml_roundtrip(tmp_path: Path) -> None:
    _require_signer_integration_enabled()
    _sdk, signer = _require_importable_modules()

    program = tmp_path / "program.yaml"
    program.write_text(
        """
app:
  network: testnet11
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/abc
  kms_region: us-west-2
  kms_public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
  coinset_msp_base_url: https://api-msp.coinset.org
vault:
  launcher_id: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"
      curve: BLS12_381
""".strip(),
        encoding="utf-8",
    )
    context: Any = signer.resolve_vault_context(str(program))
    assert context["launcher_id"] == "aa" * 32
    assert context["custody_threshold"] == 1
