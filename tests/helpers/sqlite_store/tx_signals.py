from __future__ import annotations

from tests.helpers.sqlite_store.base import SqliteStoreMixin
from tests.helpers.sqlite_store.schema import utcnow_iso


class TxSignalStoreMixin(SqliteStoreMixin):

    def observe_mempool_tx_ids(self, tx_ids: list[str]) -> int:
        if not tx_ids:
            return 0
        inserted = 0
        now = utcnow_iso()
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
        now = utcnow_iso()
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
