from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path


@dataclass(slots=True)
class StoredAlertState:
    market_id: str
    is_low: bool
    last_alert_at: datetime | None


def _utcnow_iso() -> str:
    return datetime.now(UTC).isoformat()


class SqliteStore:
    def __init__(self, db_path: Path) -> None:
        self.db_path = db_path
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self.conn = sqlite3.connect(self.db_path)
        self.conn.row_factory = sqlite3.Row
        self._init_schema()

    def close(self) -> None:
        self.conn.close()

    def _init_schema(self) -> None:
        self.conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS alert_state (
              market_id TEXT PRIMARY KEY,
              is_low INTEGER NOT NULL,
              last_alert_at TEXT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audit_event (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              event_type TEXT NOT NULL,
              market_id TEXT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS price_policy_history (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              market_id TEXT NOT NULL,
              source TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tx_signal_state (
              tx_id TEXT PRIMARY KEY,
              mempool_observed_at TEXT NOT NULL,
              tx_block_confirmed_at TEXT NULL
            );

            CREATE TABLE IF NOT EXISTS offer_state (
              offer_id TEXT PRIMARY KEY,
              market_id TEXT NOT NULL,
              state TEXT NOT NULL,
              last_seen_status INTEGER NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS coin_op_ledger (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              market_id TEXT NOT NULL,
              op_type TEXT NOT NULL,
              op_count INTEGER NOT NULL,
              fee_mojos INTEGER NOT NULL,
              status TEXT NOT NULL,
              reason TEXT NOT NULL,
              operation_id TEXT NULL,
              created_at TEXT NOT NULL
            );
            """
        )
        self.conn.commit()

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
                _utcnow_iso(),
            ),
        )
        self.conn.commit()

    def add_audit_event(self, event_type: str, payload: dict, market_id: str | None = None) -> None:
        self.conn.execute(
            """
            INSERT INTO audit_event (event_type, market_id, payload_json, created_at)
            VALUES (?, ?, ?, ?)
            """,
            (event_type, market_id, json.dumps(payload, sort_keys=True), _utcnow_iso()),
        )
        self.conn.commit()

    def add_price_policy_snapshot(
        self, market_id: str, payload: dict, source: str = "startup"
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO price_policy_history (market_id, source, payload_json, created_at)
            VALUES (?, ?, ?, ?)
            """,
            (market_id, source, json.dumps(payload, sort_keys=True), _utcnow_iso()),
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

    def observe_mempool_tx_ids(self, tx_ids: list[str]) -> int:
        if not tx_ids:
            return 0
        inserted = 0
        now = _utcnow_iso()
        for tx_id in tx_ids:
            cur = self.conn.execute(
                """
                INSERT OR IGNORE INTO tx_signal_state (tx_id, mempool_observed_at, tx_block_confirmed_at)
                VALUES (?, ?, NULL)
                """,
                (tx_id, now),
            )
            inserted += int(cur.rowcount or 0)
        self.conn.commit()
        return inserted

    def confirm_tx_ids(self, tx_ids: list[str]) -> int:
        if not tx_ids:
            return 0
        now = _utcnow_iso()
        updated = 0
        for tx_id in tx_ids:
            cur = self.conn.execute(
                """
                UPDATE tx_signal_state
                SET tx_block_confirmed_at = COALESCE(tx_block_confirmed_at, ?)
                WHERE tx_id = ?
                """,
                (now, tx_id),
            )
            updated += int(cur.rowcount or 0)
        self.conn.commit()
        return updated

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
            (offer_id, market_id, state, last_seen_status, _utcnow_iso()),
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
                "offer_id": str(r["offer_id"]),
                "market_id": str(r["market_id"]),
                "state": str(r["state"]),
                "last_seen_status": (
                    int(r["last_seen_status"]) if r["last_seen_status"] is not None else None
                ),
                "updated_at": str(r["updated_at"]),
            }
            for r in rows
        ]

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

    def add_coin_op_ledger_entry(
        self,
        *,
        market_id: str,
        op_type: str,
        op_count: int,
        fee_mojos: int,
        status: str,
        reason: str,
        operation_id: str | None,
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO coin_op_ledger
              (market_id, op_type, op_count, fee_mojos, status, reason, operation_id, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                market_id,
                op_type,
                int(op_count),
                int(fee_mojos),
                status,
                reason,
                operation_id,
                _utcnow_iso(),
            ),
        )
        self.conn.commit()

    def get_daily_fee_spent_mojos_utc(self) -> int:
        row = self.conn.execute(
            """
            SELECT COALESCE(SUM(fee_mojos), 0) AS total
            FROM coin_op_ledger
            WHERE date(created_at) = date('now')
              AND status = 'executed'
            """
        ).fetchone()
        if row is None:
            return 0
        return int(row["total"] or 0)

    def get_coin_op_budget_report_utc(self) -> dict:
        row = self.conn.execute(
            """
            SELECT
              COALESCE(SUM(CASE WHEN status = 'executed' THEN fee_mojos ELSE 0 END), 0) AS spent_mojos,
              COALESCE(SUM(CASE WHEN status = 'executed' THEN op_count ELSE 0 END), 0) AS executed_ops,
              COALESCE(SUM(CASE WHEN status = 'planned' THEN op_count ELSE 0 END), 0) AS planned_ops,
              COALESCE(SUM(CASE WHEN status = 'skipped' THEN op_count ELSE 0 END), 0) AS skipped_ops,
              COALESCE(SUM(CASE WHEN status = 'skipped' AND reason = 'fee_budget_guard' THEN op_count ELSE 0 END), 0) AS fee_budget_skipped_ops
            FROM coin_op_ledger
            WHERE date(created_at) = date('now')
            """
        ).fetchone()
        if row is None:
            return {
                "spent_mojos": 0,
                "executed_ops": 0,
                "planned_ops": 0,
                "skipped_ops": 0,
                "fee_budget_skipped_ops": 0,
            }
        return {
            "spent_mojos": int(row["spent_mojos"] or 0),
            "executed_ops": int(row["executed_ops"] or 0),
            "planned_ops": int(row["planned_ops"] or 0),
            "skipped_ops": int(row["skipped_ops"] or 0),
            "fee_budget_skipped_ops": int(row["fee_budget_skipped_ops"] or 0),
        }
