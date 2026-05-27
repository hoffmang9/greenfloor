"""Unified signing entrypoint: vault KMS via Rust signer, BLS via adapters.bls_signing."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters import bls_signing
from greenfloor.adapters import rust_signer
from greenfloor.adapters.bls_cat_coins import (
    _list_unspent_cat_coins,
    _list_unspent_cat_coins_by_ids,
)
from greenfloor.adapters.bls_signing import (
    _build_mixed_split_spend_bundle,
    _from_input_spend_bundle_xch,
    _import_sdk,
)
from greenfloor.runtime.coinset_runtime import _coinset_adapter

__all__ = [
    "build_signed_spend_bundle",
    "sign_and_broadcast",
    "sign_and_broadcast_mixed_split",
    "_build_mixed_split_spend_bundle",
    "_coinset_adapter",
    "_from_input_spend_bundle_xch",
    "_import_sdk",
    "_list_unspent_cat_coins",
    "_list_unspent_cat_coins_by_ids",
]


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
    return bls_signing.build_signed_spend_bundle(payload)


def sign_and_broadcast_mixed_split(payload: dict[str, Any]) -> dict[str, Any]:
    if rust_signer.is_vault_kms_payload(payload):
        return rust_signer.sign_and_broadcast_vault_mixed_split(payload)
    return bls_signing.sign_and_broadcast_mixed_split(payload)


def sign_and_broadcast(payload: dict[str, Any]) -> dict[str, Any]:
    return bls_signing.sign_and_broadcast(payload)
