from __future__ import annotations

from tests.helpers.sqlite_store.base import SqliteStoreMixin
from tests.helpers.sqlite_store.schema import utcnow_iso


class OfferStoreMixin(SqliteStoreMixin):
    def upsert_offer_state(
        self,
        *,
        offer_id: str,
        market_id: str,
        state: str,
        last_seen_status: int | None,
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO offer_state (offer_id, market_id, state, last_seen_status, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(offer_id) DO UPDATE SET
              market_id = excluded.market_id,
              state = excluded.state,
              last_seen_status = excluded.last_seen_status,
              updated_at = excluded.updated_at
            """,
            (offer_id, market_id, state, last_seen_status, utcnow_iso()),
        )
        self.conn.commit()

    def list_offer_states(
        self,
        *,
        market_id: str | None = None,
        limit: int = 200,
    ) -> list[dict]:
        if limit <= 0:
            return []
        if market_id:
            rows = self.conn.execute(
                """
                SELECT offer_id, market_id, state, last_seen_status, updated_at
                FROM offer_state
                WHERE market_id = ?
                ORDER BY updated_at DESC
                LIMIT ?
                """,
                (market_id, int(limit)),
            ).fetchall()
        else:
            rows = self.conn.execute(
                """
                SELECT offer_id, market_id, state, last_seen_status, updated_at
                FROM offer_state
                ORDER BY updated_at DESC
                LIMIT ?
                """,
                (int(limit),),
            ).fetchall()
        return [
            {
                "offer_id": str(row["offer_id"]),
                "market_id": str(row["market_id"]),
                "state": str(row["state"]),
                "last_seen_status": (
                    int(row["last_seen_status"]) if row["last_seen_status"] is not None else None
                ),
                "updated_at": str(row["updated_at"]),
            }
            for row in rows
        ]
