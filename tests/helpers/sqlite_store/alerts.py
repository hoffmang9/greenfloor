from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime

from tests.helpers.sqlite_store.base import SqliteStoreMixin
from tests.helpers.sqlite_store.schema import utcnow_iso


@dataclass(slots=True)
class StoredAlertState:
    market_id: str
    is_low: bool
    last_alert_at: datetime | None


class AlertStoreMixin(SqliteStoreMixin):

    def get_alert_state(self, market_id: str) -> StoredAlertState:
        row = self.conn.execute(
            "SELECT market_id, is_low, last_alert_at FROM alert_state WHERE market_id = ?",
            (market_id,),
        ).fetchone()
        if row is None:
            return StoredAlertState(market_id=market_id, is_low=False, last_alert_at=None)
        last_alert_at = (
            datetime.fromisoformat(row["last_alert_at"]) if row["last_alert_at"] else None
        )
        return StoredAlertState(
            market_id=row["market_id"],
            is_low=bool(row["is_low"]),
            last_alert_at=last_alert_at,
        )

    def upsert_alert_state(self, state: StoredAlertState) -> None:
        self.conn.execute(
            """
            INSERT INTO alert_state (market_id, is_low, last_alert_at, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(market_id) DO UPDATE SET
              is_low = excluded.is_low,
              last_alert_at = excluded.last_alert_at,
              updated_at = excluded.updated_at
            """,
            (
                state.market_id,
                1 if state.is_low else 0,
                state.last_alert_at.isoformat() if state.last_alert_at else None,
                utcnow_iso(),
            ),
        )
        self.conn.commit()
