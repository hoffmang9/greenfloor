"""Legacy offer-builder entry point; ``build_offer`` uses the Rust kernel BLS action path."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.native_offer import encode_offer_from_spend_bundle_hex
from greenfloor.adapters.offer_action import (
    build_bls_offer_for_action,
    legacy_action_request_from_payload,
)


def _build_offer(payload: dict[str, Any]) -> str:
    spend_bundle_hex = str(payload.get("spend_bundle_hex", "")).strip()
    if spend_bundle_hex:
        raw_hex = (
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
        return encode_offer_from_spend_bundle_hex(raw_hex)
    request = legacy_action_request_from_payload(payload)
    result = build_bls_offer_for_action(
        network=str(payload.get("network", "")).strip(),
        key_id=str(payload.get("key_id", "")).strip(),
        request=request,
    )
    return str(result["offer_text"])


def build_offer(payload: dict[str, Any]) -> str:
    """Build an offer1... string from payload. Raises on failure."""
    return _build_offer(payload)
