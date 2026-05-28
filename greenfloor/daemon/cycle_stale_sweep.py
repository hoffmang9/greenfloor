"""Stale open-offer sweep for daemon cycle requeue detection."""

from __future__ import annotations

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.core.cycle import (
    classify_dexie_stale_offer_status,
    collect_stale_sweep_candidates,
    empty_stale_sweep_payload,
    is_dexie_offer_missing_error_text,
    record_stale_sweep_check,
)
from greenfloor.core.cycle_orchestration import OfferStateRow, StaleSweepHit, StaleSweepProgress
from greenfloor.storage.sqlite import SqliteStore

GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET = 3
GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS = 60


def detect_stale_open_offers_for_requeue(
    *,
    store: SqliteStore,
    dexie: DexieAdapter,
    enabled_market_ids: set[str],
    per_market_limit: int = GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET,
    max_offer_checks: int = GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS,
) -> StaleSweepProgress:
    if not enabled_market_ids:
        return empty_stale_sweep_payload()

    rows = store.list_offer_states(limit=5000)
    offer_rows = [
        OfferStateRow(
            market_id=str(row.get("market_id", "")),
            offer_id=str(row.get("offer_id", "")),
            state=str(row.get("state", "")),
        )
        for row in rows
    ]
    candidates = collect_stale_sweep_candidates(
        rows=offer_rows,
        enabled_market_ids=sorted(enabled_market_ids),
        per_market_limit=max(1, int(per_market_limit)),
    )
    progress = empty_stale_sweep_payload()
    check_limit = max(1, int(max_offer_checks))
    for candidate in candidates:
        if int(progress.checked_offer_count) >= check_limit:
            return StaleSweepProgress(
                checked_offer_count=progress.checked_offer_count,
                requeue_market_ids=list(progress.requeue_market_ids),
                hits=list(progress.hits),
                truncated=True,
            )
        market_id = str(candidate.market_id).strip()
        offer_id = str(candidate.offer_id).strip()
        hit: StaleSweepHit | None = None
        try:
            payload = dexie.get_offer(offer_id, timeout=5)
            offer = payload.get("offer") if isinstance(payload, dict) else None
            if isinstance(offer, dict):
                try:
                    status = int(offer.get("status", -1))
                except (TypeError, ValueError):
                    status = -1
                reason = classify_dexie_stale_offer_status(status)
                if reason:
                    hit = StaleSweepHit(
                        market_id=market_id,
                        offer_id=offer_id,
                        reason=reason,
                    )
        except Exception as exc:  # pragma: no cover - network dependent
            if is_dexie_offer_missing_error_text(str(exc)):
                hit = StaleSweepHit(
                    market_id=market_id,
                    offer_id=offer_id,
                    reason="offer_missing_404",
                )
        progress = record_stale_sweep_check(progress=progress, hit=hit)
    return progress
