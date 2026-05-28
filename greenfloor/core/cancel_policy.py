"""Rust-backed cancel-on-strong-move policy (deterministic surface)."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.core.kernel_bridge import policy_kernel


@dataclass(frozen=True, slots=True)
class CancelPolicyDecision:
    eligible: bool
    triggered: bool
    reason: str
    move_bps: float | None
    threshold_bps: int


@dataclass(frozen=True, slots=True)
class OpenOfferRow:
    offer_id: str
    status: int


def abs_move_bps(current: float | None, previous: float | None) -> float | None:
    result = policy_kernel().abs_move_bps(current, previous)
    return None if result is None else float(result)


def cancel_move_threshold_bps(
    *,
    market_threshold: int | None = None,
    env_threshold: int | None = None,
) -> int:
    return int(policy_kernel().cancel_move_threshold_bps(market_threshold, env_threshold))


def evaluate_cancel_policy_decision(
    *,
    quote_asset_type: str,
    cancel_policy_stable_vs_unstable: bool,
    current_xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    market_threshold: int | None = None,
    env_threshold: int | None = None,
) -> CancelPolicyDecision:
    result = policy_kernel().evaluate_cancel_policy_decision(
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


def open_offer_rows_from_dicts(offers: list[dict[str, Any]]) -> list[OpenOfferRow]:
    rows: list[OpenOfferRow] = []
    for offer in offers:
        raw_status = offer.get("status", -1)
        try:
            status = int(raw_status)
        except (TypeError, ValueError):
            status = -1
        rows.append(OpenOfferRow(offer_id=str(offer.get("id", "")), status=status))
    return rows


def collect_open_offer_ids_for_cancel(offers: list[OpenOfferRow]) -> list[str]:
    result = policy_kernel().collect_open_offer_ids_for_cancel(offers)
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
