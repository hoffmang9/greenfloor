"""Per-cycle Dexie offer fetch, watchlist refresh, and lifecycle transitions."""

from __future__ import annotations

from datetime import datetime
from typing import Any, Protocol

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.core.offer_reconcile import CycleOfferTransition
from greenfloor.daemon.market_logging import _log_market_decision
from greenfloor.runtime.offer_reconciliation import reconcile_market_watched_offers
from greenfloor.storage.sqlite import SqliteStore


class _CycleReconcileResult(Protocol):
    cycle_errors: int
    immediate_requeue_requested: bool
    immediate_requeue_signals: list[str]


def reconcile_market_cycle_offers(
    *,
    market: Any,
    network: str,
    dexie: DexieAdapter,
    store: SqliteStore,
    now: datetime,
    result: _CycleReconcileResult,
) -> tuple[list[dict[str, Any]], dict[str, int], str | None, list[dict[str, Any]]]:
    """Fetch Dexie offers, augment beyond-cap offers, and transition lifecycle states."""
    market_id = str(market.market_id)

    def _on_decision(action: str, **fields: Any) -> None:
        _log_market_decision(market_id, action, **fields)

    def _on_transition(
        *,
        offer_id: str,
        transition: CycleOfferTransition,
        dexie_status: int | None = None,
        dexie_error: str | None = None,
    ) -> None:
        _log_market_decision(
            market_id,
            "offer_transition",
            offer_id=offer_id,
            dexie_status=dexie_status,
            signal_source=transition.signal_source,
            old_state=transition.old_state,
            new_state=transition.new_state,
            signal=transition.signal,
        )

    reconcile_result = reconcile_market_watched_offers(
        market=market,
        network=network,
        dexie=dexie,
        store=store,
        now=now,
        result=result,
        on_decision=_on_decision,
        on_transition=_on_transition,
    )
    return (
        reconcile_result.augmented_offers,
        reconcile_result.dexie_size_by_offer_id,
        reconcile_result.dexie_fetch_error,
        reconcile_result.offers,
    )
