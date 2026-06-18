from __future__ import annotations

import json

from tests.helpers.sqlite_store.base import SqliteStoreMixin
from tests.helpers.sqlite_store.schema import utcnow_iso


class PricingStoreMixin(SqliteStoreMixin):
    def add_price_policy_snapshot(
        self, market_id: str, payload: dict, source: str = "startup"
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO price_policy_history (market_id, source, payload_json, created_at)
            VALUES (?, ?, ?, ?)
            """,
            (market_id, source, json.dumps(payload, sort_keys=True), utcnow_iso()),
        )
        self.conn.commit()

    def get_latest_xch_price_snapshot(self) -> float | None:
        row = self.conn.execute(
            """
            SELECT payload_json
            FROM audit_event
            WHERE event_type = 'xch_price_snapshot'
            ORDER BY id DESC
            LIMIT 1
            """
        ).fetchone()
        if row is None:
            return None
        try:
            payload = json.loads(str(row["payload_json"]))
        except Exception:
            return None
        if not isinstance(payload, dict):
            return None
        raw = payload.get("price_usd")
        if raw is None:
            return None
        try:
            value = float(raw)
        except (TypeError, ValueError):
            return None
        if value <= 0:
            return None
        return value
