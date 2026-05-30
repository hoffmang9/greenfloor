from __future__ import annotations

from greenfloor.core.engine_bridge import import_engine


def extract_coin_id_hints_from_offer_text(offer_text: str) -> list[str]:
    try:
        engine = import_engine()
        hints = engine.extract_coin_id_hints_from_offer(str(offer_text))
    except Exception:
        return []
    if not isinstance(hints, list):
        return []
    return [str(value).strip().lower() for value in hints if str(value).strip()]
