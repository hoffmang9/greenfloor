"""CLI offer reconciliation via canonical Rust engine batch reconcile."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.core.engine_bridge import db_path_from_store, import_engine, require_engine_method
from greenfloor.storage.sqlite import SqliteStore

OFFER_LIFECYCLE_TRANSITION_EVENT = "offer_lifecycle_transition"

__all__ = [
    "OFFER_LIFECYCLE_TRANSITION_EVENT",
    "ReconcileBatchItem",
    "ReconcileBatchResult",
    "reconcile_offers",
]


@dataclass(slots=True)
class ReconcileBatchItem:
    offer_id: str
    market_id: str
    old_state: str
    new_state: str
    changed: bool
    last_seen_status: int | None
    reason: str
    taker_signal: str
    taker_diagnostic: str
    signal_source: str
    coinset_tx_ids: list[str]
    coinset_confirmed_tx_ids: list[str]
    coinset_mempool_tx_ids: list[str]


@dataclass(slots=True)
class ReconcileBatchResult:
    items: list[ReconcileBatchItem]
    reconciled_count: int
    changed_count: int


def _item_from_engine(raw: Any) -> ReconcileBatchItem:
    return ReconcileBatchItem(
        offer_id=str(raw.offer_id),
        market_id=str(raw.market_id),
        old_state=str(raw.old_state),
        new_state=str(raw.new_state),
        changed=bool(raw.changed),
        last_seen_status=(int(raw.last_seen_status) if raw.last_seen_status is not None else None),
        reason=str(raw.reason),
        taker_signal=str(raw.taker_signal),
        taker_diagnostic=str(raw.taker_diagnostic),
        signal_source=str(raw.signal_source),
        coinset_tx_ids=[str(value) for value in list(raw.coinset_tx_ids)],
        coinset_confirmed_tx_ids=[str(value) for value in list(raw.coinset_confirmed_tx_ids)],
        coinset_mempool_tx_ids=[str(value) for value in list(raw.coinset_mempool_tx_ids)],
    )


def reconcile_offers(
    *,
    store: SqliteStore,
    dexie_api_base: str,
    target_venue: str,
    market_id: str | None,
    limit: int,
) -> ReconcileBatchResult:
    batch_fn = require_engine_method(
        import_engine(),
        "reconcile_offers_batch",
        missing="offer reconcile batch",
    )
    batch = batch_fn(
        db_path_from_store(store),
        dexie_api_base,
        target_venue.strip().lower(),
        market_id.strip() if market_id and market_id.strip() else None,
        int(limit),
    )
    items = [_item_from_engine(item) for item in list(batch.items)]
    return ReconcileBatchResult(
        items=items,
        reconciled_count=int(batch.reconciled_count),
        changed_count=int(batch.changed_count),
    )
