from __future__ import annotations

from greenfloor.core.engine_bridge import import_engine


def extract_coin_id_hints_from_offer_text(offer_text: str) -> list[str]:
    """Decode offer coin-id hints; propagates engine decode errors."""
    engine = import_engine()
    hints = engine.extract_coin_id_hints_from_offer(str(offer_text))
    if not isinstance(hints, list):
        raise TypeError("extract_coin_id_hints_from_offer returned non-list payload")
    return [str(value).strip().lower() for value in hints if str(value).strip()]


def extract_coin_id_hints_for_logging(offer_text: str) -> list[str]:
    """Best-effort coin-id hints for log metadata (never raises)."""
    try:
        return extract_coin_id_hints_from_offer_text(offer_text)
    except Exception:
        return []


def encode_offer_from_spend_bundle_hex(raw_hex: str) -> str:
    """Encode a spend bundle hex string to offer1... via the Rust engine."""
    return str(import_engine().encode_offer(bytes.fromhex(raw_hex)))
