"""Offer codec helpers via the Rust engine (``engine_bridge.import_engine``)."""

from __future__ import annotations

from greenfloor.core.engine_bridge import import_engine


def encode_offer_from_spend_bundle_hex(raw_hex: str) -> str:
    """Encode a spend bundle hex string to offer1... via the Rust engine."""
    return str(import_engine().encode_offer(bytes.fromhex(raw_hex)))
