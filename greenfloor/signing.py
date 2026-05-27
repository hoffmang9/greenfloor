"""Unified signing entrypoint: vault KMS via Rust signer, BLS via adapters.bls_signing."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters import bls_signing as _bls
from greenfloor.adapters import rust_signer

for _name in dir(_bls):
    if _name.startswith("__"):
        continue
    globals()[_name] = getattr(_bls, _name)


def build_signed_spend_bundle(payload: dict[str, Any]) -> dict[str, Any]:
    if rust_signer.is_vault_kms_payload(payload):
        spend_bundle_hex, error = rust_signer.build_vault_offer_from_payload(payload)
        if spend_bundle_hex is None:
            return {"status": "skipped", "reason": f"signing_failed:{error}"}
        return {
            "status": "executed",
            "reason": "signing_success",
            "spend_bundle_hex": spend_bundle_hex,
        }
    return _bls.build_signed_spend_bundle(payload)


def sign_and_broadcast_mixed_split(payload: dict[str, Any]) -> dict[str, Any]:
    if rust_signer.is_vault_kms_payload(payload):
        return rust_signer.sign_and_broadcast_vault_mixed_split(payload)
    return _bls.sign_and_broadcast_mixed_split(payload)


def sign_and_broadcast(payload: dict[str, Any]) -> dict[str, Any]:
    return _bls.sign_and_broadcast(payload)
