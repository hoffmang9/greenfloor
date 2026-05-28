"""Rust-backed cancel-on-strong-move policy (deterministic surface)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.core.kernel_bridge import import_kernel


@dataclass(frozen=True, slots=True)
class CancelPolicyDecision:
    eligible: bool
    triggered: bool
    reason: str
    move_bps: float | None
    threshold_bps: int


def _parse_optional_positive_int(value: object) -> int | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, int):
        parsed = value
    elif isinstance(value, str):
        stripped = value.strip()
        if not stripped:
            return None
        try:
            parsed = int(stripped)
        except ValueError:
            return None
    else:
        return None
    if parsed <= 0:
        return None
    return parsed


def abs_move_bps(current: float | None, previous: float | None) -> float | None:
    result = import_kernel().abs_move_bps(current, previous)
    return None if result is None else float(result)


def cancel_move_threshold_bps(
    *,
    market_threshold_raw: object = None,
    env_raw: str = "",
) -> int:
    market_threshold = _parse_optional_positive_int(market_threshold_raw)
    env_threshold = _parse_optional_positive_int(env_raw.strip()) if env_raw.strip() else None
    return int(import_kernel().cancel_move_threshold_bps(market_threshold, env_threshold))


def evaluate_cancel_policy_decision(
    *,
    quote_asset_type: str,
    cancel_policy_stable_vs_unstable: bool,
    current_xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    market_threshold_raw: object = None,
    env_raw: str = "",
) -> CancelPolicyDecision:
    market_threshold = _parse_optional_positive_int(market_threshold_raw)
    env_threshold = _parse_optional_positive_int(env_raw.strip()) if env_raw.strip() else None
    payload = import_kernel().evaluate_cancel_policy_decision(
        str(quote_asset_type),
        bool(cancel_policy_stable_vs_unstable),
        current_xch_price_usd,
        previous_xch_price_usd,
        market_threshold,
        env_threshold,
    )
    move_bps_raw = payload.get("move_bps")
    move_bps = None if move_bps_raw is None else float(move_bps_raw)
    return CancelPolicyDecision(
        eligible=bool(payload["eligible"]),
        triggered=bool(payload["triggered"]),
        reason=str(payload["reason"]),
        move_bps=move_bps,
        threshold_bps=int(payload["threshold_bps"]),
    )


def collect_open_offer_ids_for_cancel(offers: list[dict[str, Any]]) -> list[str]:
    return [str(offer_id) for offer_id in import_kernel().collect_open_offer_ids_for_cancel(offers)]
