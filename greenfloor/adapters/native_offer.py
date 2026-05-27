"""Native (``greenfloor_native``) offer helpers — not part of the BLS Rust signer path."""

from __future__ import annotations

import importlib
from typing import Any


def _import_greenfloor_native() -> Any:
    return importlib.import_module("greenfloor_native")


def encode_offer_from_spend_bundle_hex(raw_hex: str) -> str | None:
    """Return offer1... text when native encode succeeds, else None (caller uses SDK fallback)."""
    try:
        native = _import_greenfloor_native()
    except ImportError:
        return None
    encode = getattr(native, "encode_offer", None)
    if not callable(encode):
        return None
    try:
        return str(encode(bytes.fromhex(raw_hex)))
    except Exception:
        return None


def _as_bytes(value: Any) -> bytes:
    if isinstance(value, bytes | bytearray | memoryview):
        return bytes(value)
    to_bytes = getattr(value, "to_bytes", None)
    if callable(to_bytes):
        raw = to_bytes()
        if isinstance(raw, bytes | bytearray | memoryview):
            return bytes(raw)
        raise TypeError("to_bytes did not return bytes-compatible data")
    to_dunder_bytes = getattr(value, "__bytes__", None)
    if callable(to_dunder_bytes):
        raw = to_dunder_bytes()
        if isinstance(raw, bytes | bytearray | memoryview):
            return bytes(raw)
        raise TypeError("__bytes__ did not return bytes-compatible data")
    raise TypeError("value cannot be converted to bytes")


def from_input_spend_bundle_xch(
    *,
    sdk: Any,
    input_spend_bundle: Any,
    requested_payments_xch: list[Any],
) -> Any:
    native = _import_greenfloor_native()
    requested: list[tuple[bytes, list[tuple[bytes, int]]]] = []
    for notarized_payment in requested_payments_xch:
        payments: list[tuple[bytes, int]] = []
        for payment in notarized_payment.payments:
            payments.append((_as_bytes(payment.puzzle_hash), int(payment.amount)))
        requested.append((_as_bytes(notarized_payment.nonce), payments))
    spend_bundle_bytes = native.from_input_spend_bundle_xch(
        input_spend_bundle.to_bytes(),
        requested,
    )
    return sdk.SpendBundle.from_bytes(spend_bundle_bytes)
