"""Daemon cancel-on-strong-move policy."""

from __future__ import annotations

import os
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.core.cancel_policy import (
    cancel_policy_audit_payload,
    collect_open_offer_ids_for_cancel,
    evaluate_cancel_policy_decision,
)
from greenfloor.daemon.cooldowns import (
    _CANCEL_COOLDOWN_UNTIL,
    _cancel_offer_with_retry,
    _cancel_retry_config,
    _cooldown_remaining_ms,
    _set_cooldown,
)
from greenfloor.daemon.market_helpers import _market_pricing
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
    pricing = _market_pricing(market)
    decision = evaluate_cancel_policy_decision(
        quote_asset_type=str(market.quote_asset_type),
        cancel_policy_stable_vs_unstable=bool(
            pricing.get("cancel_policy_stable_vs_unstable", False)
        ),
        current_xch_price_usd=current_xch_price_usd,
        previous_xch_price_usd=previous_xch_price_usd,
        market_threshold_raw=pricing.get("cancel_move_threshold_bps"),
        env_raw=os.getenv("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS", "").strip(),
    )
    if not decision.triggered:
        return cancel_policy_audit_payload(decision)

    target_offer_ids = collect_open_offer_ids_for_cancel(offers)
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

    return cancel_policy_audit_payload(
        decision,
        planned_count=len(target_offer_ids),
        executed_count=executed_count,
        items=items,
    )
