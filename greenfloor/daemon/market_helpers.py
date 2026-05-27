"""Shared market pricing and cancel-policy helpers."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from greenfloor.config.io import default_cats_config_path, resolve_quote_asset_for_offer
from greenfloor.hex_utils import default_mojo_multiplier_for_asset, is_hex_id

_DEFAULT_CANCEL_MOVE_THRESHOLD_BPS = 500


def _normalize_strategy_pair(quote_asset: str) -> str:
    lowered = quote_asset.strip().lower()
    if lowered == "xch":
        return "xch"
    if "usdc" in lowered:
        return "usdc"
    return lowered


def _is_hex_asset_id(value: str) -> bool:
    return is_hex_id(value)


def _default_cats_config_path() -> Path | None:
    return default_cats_config_path()


def _cancel_move_threshold_bps(*, market: Any | None = None) -> int:
    pricing = dict(getattr(market, "pricing", {}) or {}) if market is not None else {}
    threshold_raw = pricing.get("cancel_move_threshold_bps")
    if threshold_raw is not None:
        try:
            parsed_threshold = int(threshold_raw)
        except (TypeError, ValueError):
            parsed_threshold = 0
        if parsed_threshold > 0:
            return parsed_threshold
    raw = os.getenv("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS", "").strip()
    if not raw:
        return _DEFAULT_CANCEL_MOVE_THRESHOLD_BPS
    try:
        parsed = int(raw)
    except ValueError:
        return _DEFAULT_CANCEL_MOVE_THRESHOLD_BPS
    return max(1, parsed)


def _abs_move_bps(current: float | None, previous: float | None) -> float | None:
    if current is None or previous is None:
        return None
    if current <= 0 or previous <= 0:
        return None
    return abs((current - previous) / previous) * 10_000.0


def _resolve_quote_asset_for_offer(*, quote_asset: str, network: str) -> str:
    return resolve_quote_asset_for_offer(quote_asset=quote_asset, network=network)


def _market_pricing(market: Any) -> dict[str, Any]:
    return dict(getattr(market, "pricing", {}) or {})


def _normalize_offer_side(value: Any) -> str:
    side = str(value or "").strip().lower()
    return "buy" if side == "buy" else "sell"


def _base_unit_mojo_multiplier_for_market(*, market: Any) -> int:
    pricing = getattr(market, "pricing", {}) or {}
    default_multiplier = default_mojo_multiplier_for_asset(str(getattr(market, "base_asset", "")))
    try:
        multiplier = int(pricing.get("base_unit_mojo_multiplier", default_multiplier))
    except (TypeError, ValueError):
        multiplier = default_multiplier
    return max(1, multiplier)
