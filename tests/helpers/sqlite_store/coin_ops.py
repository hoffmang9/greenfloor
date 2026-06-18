from __future__ import annotations

from tests.helpers.sqlite_store.base import SqliteStoreMixin
from tests.helpers.sqlite_store.schema import utcnow_iso


class CoinOpStoreMixin(SqliteStoreMixin):
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
                utcnow_iso(),
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
