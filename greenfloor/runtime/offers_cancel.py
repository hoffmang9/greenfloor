"""Venue offer cancel selection and execution (Dexie-first)."""

from __future__ import annotations

from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.storage.sqlite import SqliteStore


def select_offers_for_cancel(
    *,
    store: SqliteStore,
    offer_ids: list[str],
    cancel_open: bool,
    market_id: str | None = None,
) -> list[dict[str, str]]:
    rows = store.list_offer_states(market_id=market_id, limit=500)
    normalized = [
        {
            "offer_id": str(row.get("offer_id", "")).strip(),
            "market_id": str(row.get("market_id", "")).strip(),
            "state": str(row.get("state", "")).strip().lower(),
            "last_seen_status": str(row.get("last_seen_status", "")),
        }
        for row in rows
        if str(row.get("offer_id", "")).strip()
    ]
    if cancel_open:
        return [row for row in normalized if row["state"] == "open"]
    requested_ids = {str(value).strip() for value in offer_ids if str(value).strip()}
    if not requested_ids:
        raise ValueError("provide at least one --offer-id or pass --cancel-open")
    return [row for row in normalized if row["offer_id"] in requested_ids]


def cancel_offers_on_venue(
    *,
    dexie: DexieAdapter,
    store: SqliteStore,
    selected_offers: list[dict[str, str]],
) -> tuple[list[dict[str, Any]], int]:
    items: list[dict[str, Any]] = []
    failures = 0
    for row in selected_offers:
        offer_id = row["offer_id"]
        try:
            result = dexie.cancel_offer(offer_id)
            success = bool(result.get("success", False))
            if success:
                store.upsert_offer_state(
                    offer_id=offer_id,
                    market_id=str(row.get("market_id", "")).strip() or "unknown",
                    state="cancelled",
                    last_seen_status=3,
                )
            else:
                failures += 1
            items.append(
                {
                    "offer_id": offer_id,
                    "market_id": row.get("market_id", ""),
                    "state": row.get("state", ""),
                    "result": {
                        "success": success,
                        "venue_response": result,
                        "error": str(result.get("error", "")).strip() if not success else "",
                    },
                }
            )
        except Exception as exc:
            failures += 1
            items.append(
                {
                    "offer_id": offer_id,
                    "market_id": row.get("market_id", ""),
                    "state": row.get("state", ""),
                    "result": {"success": False, "error": str(exc)},
                }
            )
    return items, failures
