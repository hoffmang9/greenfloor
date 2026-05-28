"""Shared market pricing and cancel-policy helpers."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor.config.io import default_cats_config_path, resolve_quote_asset_for_offer
from greenfloor.core.cancel_policy import abs_move_bps, cancel_move_threshold_bps
from greenfloor.core.threshold_parsing import (
    market_cancel_move_threshold_bps,
    unstable_cancel_move_threshold_bps_from_env,
)
from greenfloor.hex_utils import default_mojo_multiplier_for_asset, is_hex_id


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
    market_threshold = market_cancel_move_threshold_bps(market) if market is not None else None
    return cancel_move_threshold_bps(
        market_threshold=market_threshold,
        env_threshold=unstable_cancel_move_threshold_bps_from_env(),
    )


def _abs_move_bps(current: float | None, previous: float | None) -> float | None:
    return abs_move_bps(current, previous)


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
