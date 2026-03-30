from __future__ import annotations

import importlib

from greenfloor.hex_utils import normalize_hex_id


def extract_coin_id_hints_from_offer_text(offer_text: str) -> list[str]:
    try:
        sdk = importlib.import_module("chia_wallet_sdk")
    except Exception:
        return []
    decode_offer = getattr(sdk, "decode_offer", None)
    if not callable(decode_offer):
        return []
    try:
        spend_bundle = decode_offer(offer_text)
    except Exception:
        return []
    coin_spends = getattr(spend_bundle, "coin_spends", None) or []
    hints: list[str] = []
    for coin_spend in coin_spends:
        coin = getattr(coin_spend, "coin", None)
        if coin is None:
            continue
        coin_id_fn = getattr(coin, "coin_id", None)
        if not callable(coin_id_fn):
            continue
        try:
            coin_id_obj = coin_id_fn()
            to_hex = getattr(sdk, "to_hex", None)
            if not callable(to_hex):
                continue
            coin_id_hex = str(to_hex(coin_id_obj)).strip().lower()
        except Exception:
            continue
        normalized = normalize_hex_id(coin_id_hex)
        if normalized:
            hints.append(normalized)
    # Stable order, unique values.
    return list(dict.fromkeys(hints))
