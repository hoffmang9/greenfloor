"""Populate SqliteStore fixtures for Rust-backed watchlist tests."""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import Any

from greenfloor.storage.sqlite import SqliteStore


def open_watchlist_test_store(tmp_path: Path) -> SqliteStore:
    db_path = tmp_path / "watchlist.sqlite"
    return SqliteStore(db_path)


def seed_offer_state(
    store: SqliteStore,
    *,
    offer_id: str,
    market_id: str,
    state: str,
    updated_at: datetime | None = None,
) -> None:
    ts = (updated_at or datetime.now()).isoformat()
    store.conn.execute(
        """
        INSERT INTO offer_state (offer_id, market_id, state, last_seen_status, updated_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(offer_id) DO UPDATE SET
          market_id = excluded.market_id,
          state = excluded.state,
          last_seen_status = excluded.last_seen_status,
          updated_at = excluded.updated_at
        """,
        (offer_id, market_id, state, None, ts),
    )
    store.conn.commit()


def seed_strategy_execution_event(
    store: SqliteStore,
    *,
    market_id: str,
    items: list[dict[str, Any]],
    created_at: datetime | None = None,
    extra_payload: dict[str, Any] | None = None,
) -> None:
    import json

    payload: dict[str, Any] = {"items": items}
    if extra_payload:
        payload.update(extra_payload)
    ts = (created_at or datetime.now()).isoformat()
    store.conn.execute(
        """
        INSERT INTO audit_event (event_type, market_id, payload_json, created_at)
        VALUES (?, ?, ?, ?)
        """,
        (
            "strategy_offer_execution",
            market_id,
            json.dumps(payload),
            ts,
        ),
    )
    store.conn.commit()
