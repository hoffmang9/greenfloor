from __future__ import annotations

import json

from tests.helpers.sqlite_store.base import SqliteStoreMixin
from tests.helpers.sqlite_store.schema import utcnow_iso


class AuditStoreMixin(SqliteStoreMixin):

    def add_audit_event(self, event_type: str, payload: dict, market_id: str | None = None) -> None:
        self.conn.execute(
            """
            INSERT INTO audit_event (event_type, market_id, payload_json, created_at)
            VALUES (?, ?, ?, ?)
            """,
            (event_type, market_id, json.dumps(payload, sort_keys=True), utcnow_iso()),
        )
        self.conn.commit()

    def list_recent_audit_events(
        self,
        *,
        event_types: list[str] | None = None,
        market_id: str | None = None,
        limit: int = 50,
    ) -> list[dict]:
        if limit <= 0:
            return []
        where_clauses: list[str] = []
        params: list[object] = []
        if event_types:
            placeholders = ",".join("?" for _ in event_types)
            where_clauses.append(f"event_type IN ({placeholders})")
            params.extend(event_types)
        if market_id:
            where_clauses.append("market_id = ?")
            params.append(market_id)
        where_sql = ""
        if where_clauses:
            where_sql = "WHERE " + " AND ".join(where_clauses)
        rows = self.conn.execute(
            f"""
            SELECT id, event_type, market_id, payload_json, created_at
            FROM audit_event
            {where_sql}
            ORDER BY id DESC
            LIMIT ?
            """,
            [*params, int(limit)],
        ).fetchall()
        events: list[dict] = []
        for row in rows:
            payload: dict | list | str | int | float | bool | None
            try:
                payload = json.loads(str(row["payload_json"]))
            except Exception:
                payload = str(row["payload_json"])
            events.append(
                {
                    "id": int(row["id"]),
                    "event_type": str(row["event_type"]),
                    "market_id": str(row["market_id"]) if row["market_id"] is not None else None,
                    "payload": payload,
                    "created_at": str(row["created_at"]),
                }
            )
        return events
