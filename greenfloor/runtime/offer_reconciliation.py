"""CLI offer reconciliation via canonical Rust engine batch reconcile."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.storage.sqlite import SqliteStore

OFFER_LIFECYCLE_TRANSITION_EVENT = "offer_lifecycle_transition"

__all__ = [
    "OFFER_LIFECYCLE_TRANSITION_EVENT",
    "ReconcileBatchResult",
    "reconcile_offers",
]


@dataclass(slots=True)
class ReconcileBatchResult:
    items: list[dict[str, Any]]
    reconciled_count: int
    changed_count: int


def _db_path(store: SqliteStore) -> str:
    db_path = getattr(store, "db_path", None)
    if db_path is None:
        raise TypeError("reconcile_offers requires SqliteStore with db_path")
    return str(db_path)


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
    payload = batch_fn(
        _db_path(store),
        dexie_api_base,
        target_venue.strip().lower(),
        market_id.strip() if market_id and market_id.strip() else None,
        int(limit),
    )
    if not isinstance(payload, dict):
        raise TypeError("engine reconcile_offers_batch returned non-dict payload")
    items = payload.get("items", [])
    if not isinstance(items, list):
        raise TypeError("engine reconcile_offers_batch items is not a list")
    return ReconcileBatchResult(
        items=[dict(item) for item in items if isinstance(item, dict)],
        reconciled_count=int(payload.get("reconciled_count", 0)),
        changed_count=int(payload.get("changed_count", 0)),
    )
