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
        # Parallel market workers open independent connections. Use a non-zero
        # lock wait so short write-contention windows do not fail immediately.
        self.conn = sqlite3.connect(self.db_path, timeout=30.0)
        self.conn.row_factory = sqlite3.Row
        self.conn.execute("PRAGMA busy_timeout = 30000")
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

            CREATE TABLE IF NOT EXISTS offer_reservation_lease (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              reservation_id TEXT NOT NULL,
              market_id TEXT NOT NULL,
              wallet_id TEXT NOT NULL,
              asset_id TEXT NOT NULL,
              amount INTEGER NOT NULL,
              status TEXT NOT NULL,
              created_at TEXT NOT NULL,
              expires_at TEXT NOT NULL,
              released_at TEXT NULL
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

    def get_tx_signal_state(self, tx_ids: list[str]) -> dict[str, dict[str, str | None]]:
        if not tx_ids:
            return {}
        unique_ids: list[str] = []
        for tx_id in tx_ids:
            normalized = str(tx_id).strip()
            if not normalized:
                continue
            if normalized not in unique_ids:
                unique_ids.append(normalized)
        if not unique_ids:
            return {}
        placeholders = ",".join("?" for _ in unique_ids)
        rows = self.conn.execute(
            f"""
            SELECT tx_id, mempool_observed_at, tx_block_confirmed_at
            FROM tx_signal_state
            WHERE tx_id IN ({placeholders})
            """,
            unique_ids,
        ).fetchall()
        state_by_tx_id: dict[str, dict[str, str | None]] = {}
        for row in rows:
            key = str(row["tx_id"])
            state_by_tx_id[key] = {
                "mempool_observed_at": (
                    str(row["mempool_observed_at"])
                    if row["mempool_observed_at"] is not None
                    else None
                ),
                "tx_block_confirmed_at": (
                    str(row["tx_block_confirmed_at"])
                    if row["tx_block_confirmed_at"] is not None
                    else None
                ),
            }
        return state_by_tx_id

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

    def add_offer_reservation_lease(
        self,
        *,
        reservation_id: str,
        market_id: str,
        wallet_id: str,
        asset_amounts: dict[str, int],
        lease_seconds: int,
    ) -> None:
        if not reservation_id:
            raise ValueError("reservation_id is required")
        if lease_seconds <= 0:
            raise ValueError("lease_seconds must be > 0")
        if not asset_amounts:
            raise ValueError("asset_amounts must be non-empty")
        created_at = _utcnow_iso()
        expires_at = datetime.now(UTC).timestamp() + float(lease_seconds)
        expires_at_iso = datetime.fromtimestamp(expires_at, UTC).isoformat()
        rows = [
            (
                reservation_id,
                market_id,
                wallet_id,
                str(asset_id),
                int(amount),
                "active",
                created_at,
                expires_at_iso,
                None,
            )
            for asset_id, amount in asset_amounts.items()
            if int(amount) > 0
        ]
        if not rows:
            raise ValueError("asset_amounts must contain positive amounts")
        self.conn.executemany(
            """
            INSERT INTO offer_reservation_lease
              (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            rows,
        )
        self.conn.commit()

    def try_acquire_offer_reservation_lease(
        self,
        *,
        reservation_id: str,
        market_id: str,
        wallet_id: str,
        requested_amounts: dict[str, int],
        available_amounts: dict[str, int],
        lease_seconds: int,
        now: datetime | None = None,
    ) -> str | None:
        if not reservation_id:
            raise ValueError("reservation_id is required")
        if lease_seconds <= 0:
            raise ValueError("lease_seconds must be > 0")
        normalized_requests = {
            str(asset_id).strip().lower(): int(amount)
            for asset_id, amount in requested_amounts.items()
            if int(amount) > 0
        }
        if not normalized_requests:
            return "reservation_empty_request"
        normalized_available = {
            str(asset_id).strip().lower(): int(amount)
            for asset_id, amount in available_amounts.items()
            if int(amount) > 0
        }
        now_dt = now or datetime.now(UTC)
        now_iso = now_dt.isoformat()
        expires_at_iso = datetime.fromtimestamp(
            now_dt.timestamp() + float(lease_seconds), UTC
        ).isoformat()
        created_at_iso = now_iso
        try:
            self.conn.execute("BEGIN IMMEDIATE")
            self.conn.execute(
                """
                UPDATE offer_reservation_lease
                SET status = 'expired',
                    released_at = COALESCE(released_at, ?)
                WHERE status = 'active'
                  AND expires_at <= ?
                """,
                (now_iso, now_iso),
            )
            rows = self.conn.execute(
                """
                SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
                FROM offer_reservation_lease
                WHERE wallet_id = ?
                  AND status = 'active'
                  AND expires_at > ?
                GROUP BY asset_id
                """,
                (wallet_id, now_iso),
            ).fetchall()
            reserved_by_asset = {
                str(row["asset_id"]).strip().lower(): int(row["reserved_amount"] or 0)
                for row in rows
            }
            for asset_id, amount in normalized_requests.items():
                available = int(normalized_available.get(asset_id, 0))
                already_reserved = int(reserved_by_asset.get(asset_id, 0))
                if available - already_reserved < amount:
                    self.conn.rollback()
                    return (
                        f"reservation_insufficient_{asset_id}:"
                        f"available={available}:reserved={already_reserved}:needed={amount}"
                    )
            insert_rows = [
                (
                    reservation_id,
                    market_id,
                    wallet_id,
                    str(asset_id),
                    int(amount),
                    "active",
                    created_at_iso,
                    expires_at_iso,
                    None,
                )
                for asset_id, amount in normalized_requests.items()
            ]
            self.conn.executemany(
                """
                INSERT INTO offer_reservation_lease
                  (reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                insert_rows,
            )
            self.conn.commit()
            return None
        except Exception:
            self.conn.rollback()
            raise

    def release_offer_reservation_lease(
        self,
        *,
        reservation_id: str,
        release_status: str,
    ) -> int:
        released_at = _utcnow_iso()
        cur = self.conn.execute(
            """
            UPDATE offer_reservation_lease
            SET status = ?, released_at = ?
            WHERE reservation_id = ?
              AND status = 'active'
            """,
            (release_status, released_at, reservation_id),
        )
        self.conn.commit()
        return int(cur.rowcount or 0)

    def expire_offer_reservation_leases(self, *, now: datetime | None = None) -> int:
        now_iso = (now or datetime.now(UTC)).isoformat()
        cur = self.conn.execute(
            """
            UPDATE offer_reservation_lease
            SET status = 'expired',
                released_at = COALESCE(released_at, ?)
            WHERE status = 'active'
              AND expires_at <= ?
            """,
            (now_iso, now_iso),
        )
        self.conn.commit()
        return int(cur.rowcount or 0)

    def prune_offer_reservation_leases(self, *, older_than: datetime) -> int:
        cutoff_iso = older_than.astimezone(UTC).isoformat()
        cur = self.conn.execute(
            """
            DELETE FROM offer_reservation_lease
            WHERE status != 'active'
              AND COALESCE(released_at, expires_at) < ?
            """,
            (cutoff_iso,),
        )
        self.conn.commit()
        return int(cur.rowcount or 0)

    def get_offer_reserved_amounts_by_asset(self, *, wallet_id: str) -> dict[str, int]:
        rows = self.conn.execute(
            """
            SELECT asset_id, COALESCE(SUM(amount), 0) AS reserved_amount
            FROM offer_reservation_lease
            WHERE wallet_id = ?
              AND status = 'active'
              AND expires_at > ?
            GROUP BY asset_id
            """,
            (wallet_id, _utcnow_iso()),
        ).fetchall()
        return {str(row["asset_id"]): int(row["reserved_amount"] or 0) for row in rows}

    def list_offer_reservation_leases(
        self,
        *,
        reservation_id: str | None = None,
        include_inactive: bool = True,
    ) -> list[dict[str, str | int | None]]:
        where_clauses: list[str] = []
        params: list[object] = []
        if reservation_id:
            where_clauses.append("reservation_id = ?")
            params.append(reservation_id)
        if not include_inactive:
            where_clauses.append("status = 'active'")
            where_clauses.append("expires_at > ?")
            params.append(_utcnow_iso())
        where_sql = ""
        if where_clauses:
            where_sql = "WHERE " + " AND ".join(where_clauses)
        rows = self.conn.execute(
            f"""
            SELECT reservation_id, market_id, wallet_id, asset_id, amount, status, created_at, expires_at, released_at
            FROM offer_reservation_lease
            {where_sql}
            ORDER BY id ASC
            """,
            params,
        ).fetchall()
        return [
            {
                "reservation_id": str(row["reservation_id"]),
                "market_id": str(row["market_id"]),
                "wallet_id": str(row["wallet_id"]),
                "asset_id": str(row["asset_id"]),
                "amount": int(row["amount"] or 0),
                "status": str(row["status"]),
                "created_at": str(row["created_at"]),
                "expires_at": str(row["expires_at"]),
                "released_at": str(row["released_at"]) if row["released_at"] is not None else None,
            }
            for row in rows
        ]
