"""Offer codec helpers via ``greenfloor_signer`` (merged from greenfloor-native)."""

from __future__ import annotations

import importlib
from typing import Any


def _import_greenfloor_signer() -> Any:
    return importlib.import_module("greenfloor_signer")


def encode_offer_from_spend_bundle_hex(raw_hex: str) -> str:
    """Encode a spend bundle hex string to offer1... via ``greenfloor_signer``."""
    return str(_import_greenfloor_signer().encode_offer(bytes.fromhex(raw_hex)))


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
    signer = _import_greenfloor_signer()
    requested: list[tuple[bytes, list[tuple[bytes, int]]]] = []
    for notarized_payment in requested_payments_xch:
        payments: list[tuple[bytes, int]] = []
        for payment in notarized_payment.payments:
            payments.append((_as_bytes(payment.puzzle_hash), int(payment.amount)))
        requested.append((_as_bytes(notarized_payment.nonce), payments))
    spend_bundle_bytes = signer.from_input_spend_bundle_xch(
        input_spend_bundle.to_bytes(),
        requested,
    )
    return sdk.SpendBundle.from_bytes(spend_bundle_bytes)
