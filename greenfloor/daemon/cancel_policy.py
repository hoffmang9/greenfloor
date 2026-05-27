"""Daemon cancel-on-strong-move policy."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.daemon.cooldowns import (
    _CANCEL_COOLDOWN_UNTIL,
    _cancel_offer_with_retry,
    _cancel_retry_config,
    _cooldown_remaining_ms,
    _set_cooldown,
)
from greenfloor.daemon.market_helpers import (
    _abs_move_bps,
    _cancel_move_threshold_bps,
    _market_pricing,
)
from greenfloor.storage.sqlite import SqliteStore


def _execute_cancel_policy_for_market(
    *,
    market,
    offers: list[dict[str, Any]],
    runtime_dry_run: bool,
    current_xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    dexie: DexieAdapter,
    store: SqliteStore,
) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    move_bps = _abs_move_bps(current_xch_price_usd, previous_xch_price_usd)
    quote_type = str(market.quote_asset_type).strip().lower()
    pricing = _market_pricing(market)
    stable_vs_unstable = bool(pricing.get("cancel_policy_stable_vs_unstable", False))
    threshold_bps = _cancel_move_threshold_bps(market=market)
    if quote_type != "unstable":
        return {
            "eligible": False,
            "triggered": False,
            "reason": "not_unstable_leg_market",
            "move_bps": move_bps,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }
    if not stable_vs_unstable:
        return {
            "eligible": False,
            "triggered": False,
            "reason": "not_stable_vs_unstable_market",
            "move_bps": move_bps,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }
    if move_bps is None:
        return {
            "eligible": True,
            "triggered": False,
            "reason": "missing_price_baseline",
            "move_bps": None,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }
    if move_bps < float(threshold_bps):
        return {
            "eligible": True,
            "triggered": False,
            "reason": "price_move_below_threshold",
            "move_bps": move_bps,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }

    target_offer_ids: list[str] = []
    for offer in offers:
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id:
            continue
        status = int(offer.get("status", -1))
        if status == 0:
            target_offer_ids.append(offer_id)

    executed_count = 0
    _, _, cooldown_seconds = _cancel_retry_config()
    cooldown_key = f"cancel:{market.market_id}"
    for offer_id in target_offer_ids:
        if runtime_dry_run:
            items.append({"offer_id": offer_id, "status": "planned", "reason": "dry_run"})
            continue

        remaining_ms = _cooldown_remaining_ms(_CANCEL_COOLDOWN_UNTIL, cooldown_key)
        if remaining_ms > 0:
            items.append(
                {
                    "offer_id": offer_id,
                    "status": "skipped",
                    "reason": f"cancel_cooldown_active:{remaining_ms}ms",
                }
            )
            continue
        result, attempt_count, cancel_error = _cancel_offer_with_retry(
            dexie=dexie,
            offer_id=offer_id,
        )
        success = bool(result.get("success", False))
        if success:
            executed_count += 1
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id=market.market_id,
                state="cancelled",
                last_seen_status=3,
            )
            items.append(
                {
                    "offer_id": offer_id,
                    "status": "executed",
                    "reason": "cancelled_on_strong_unstable_move",
                    "attempts": attempt_count,
                }
            )
        else:
            _set_cooldown(_CANCEL_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
            items.append(
                {
                    "offer_id": offer_id,
                    "status": "skipped",
                    "reason": f"cancel_retry_exhausted:{cancel_error}",
                    "attempts": attempt_count,
                }
            )

    return {
        "eligible": True,
        "triggered": True,
        "reason": "strong_unstable_price_move",
        "move_bps": move_bps,
        "threshold_bps": threshold_bps,
        "planned_count": len(target_offer_ids),
        "executed_count": executed_count,
        "items": items,
    }
