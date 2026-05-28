"""Rust-backed cancel-on-strong-move policy (deterministic surface)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.core.kernel_bridge import signer_kernel
from greenfloor.core.threshold_parsing import parse_cancel_move_thresholds


@dataclass(frozen=True, slots=True)
class CancelPolicyDecision:
    eligible: bool
    triggered: bool
    reason: str
    move_bps: float | None
    threshold_bps: int


def abs_move_bps(current: float | None, previous: float | None) -> float | None:
    result = signer_kernel().abs_move_bps(current, previous)
    return None if result is None else float(result)


def cancel_move_threshold_bps(
    *,
    market_threshold_raw: object = None,
    env_raw: str = "",
) -> int:
    market_threshold, env_threshold = parse_cancel_move_thresholds(
        market_threshold_raw=market_threshold_raw,
        env_raw=env_raw,
    )
    return int(signer_kernel().cancel_move_threshold_bps(market_threshold, env_threshold))


def evaluate_cancel_policy_decision(
    *,
    quote_asset_type: str,
    cancel_policy_stable_vs_unstable: bool,
    current_xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    market_threshold_raw: object = None,
    env_raw: str = "",
) -> CancelPolicyDecision:
    market_threshold, env_threshold = parse_cancel_move_thresholds(
        market_threshold_raw=market_threshold_raw,
        env_raw=env_raw,
    )
    result = signer_kernel().evaluate_cancel_policy_decision(
        str(quote_asset_type),
        bool(cancel_policy_stable_vs_unstable),
        current_xch_price_usd,
        previous_xch_price_usd,
        market_threshold,
        env_threshold,
    )
    if not isinstance(result, CancelPolicyDecision):
        raise TypeError("evaluate_cancel_policy_decision returned non-CancelPolicyDecision result")
    return result


def collect_open_offer_ids_for_cancel(offers: list[dict[str, Any]]) -> list[str]:
    result = signer_kernel().collect_open_offer_ids_for_cancel(offers)
    if not isinstance(result, list):
        raise TypeError("collect_open_offer_ids_for_cancel returned non-list result")
    return [str(offer_id) for offer_id in result]


def cancel_policy_audit_payload(
    decision: CancelPolicyDecision,
    *,
    planned_count: int = 0,
    executed_count: int = 0,
    items: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    return {
        "eligible": decision.eligible,
        "triggered": decision.triggered,
        "reason": decision.reason,
        "move_bps": decision.move_bps,
        "threshold_bps": decision.threshold_bps,
        "planned_count": planned_count,
        "executed_count": executed_count,
        "items": list(items or []),
    }
