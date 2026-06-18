use rusqlite::params;

use crate::error::{SignerError, SignerResult};

use super::{utcnow_iso, SqliteStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoinOpBudgetReport {
    pub spent_mojos: i64,
    pub executed_ops: i64,
    pub planned_ops: i64,
    pub skipped_ops: i64,
    pub fee_budget_skipped_ops: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoinOpLedgerEntry<'a> {
    pub market_id: &'a str,
    pub op_type: &'a str,
    pub op_count: i64,
    pub fee_mojos: i64,
    pub status: &'a str,
    pub reason: &'a str,
    pub operation_id: Option<&'a str>,
}

impl SqliteStore {
    pub fn get_daily_fee_spent_mojos_utc(&self) -> SignerResult<i64> {
        let total: i64 = self
            .conn
            .query_row(
                r#"
                SELECT COALESCE(SUM(fee_mojos), 0)
                FROM coin_op_ledger
                WHERE date(created_at) = date('now')
                  AND status = 'executed'
                "#,
                [],
                |row| row.get(0),
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to query daily coin-op fee total: {err}"))
            })?;
        Ok(total)
    }

    pub fn add_coin_op_ledger_entry(&self, entry: CoinOpLedgerEntry<'_>) -> SignerResult<()> {
        let CoinOpLedgerEntry {
            market_id,
            op_type,
            op_count,
            fee_mojos,
            status,
            reason,
            operation_id,
        } = entry;
        self.conn
            .execute(
                r#"
                INSERT INTO coin_op_ledger
                  (market_id, op_type, op_count, fee_mojos, status, reason, operation_id, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                params![
                    market_id,
                    op_type,
                    op_count,
                    fee_mojos,
                    status,
                    reason,
                    operation_id,
                    utcnow_iso(),
                ],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to insert coin_op_ledger row: {err}"))
            })?;
        Ok(())
    }

    pub fn get_coin_op_budget_report_utc(&self) -> SignerResult<CoinOpBudgetReport> {
        self.conn
            .query_row(
                r#"
                SELECT
                  COALESCE(SUM(CASE WHEN status = 'executed' THEN fee_mojos ELSE 0 END), 0),
                  COALESCE(SUM(CASE WHEN status = 'executed' THEN op_count ELSE 0 END), 0),
                  COALESCE(SUM(CASE WHEN status = 'planned' THEN op_count ELSE 0 END), 0),
                  COALESCE(SUM(CASE WHEN status = 'skipped' THEN op_count ELSE 0 END), 0),
                  COALESCE(SUM(CASE WHEN status = 'skipped' AND reason = 'fee_budget_guard' THEN op_count ELSE 0 END), 0)
                FROM coin_op_ledger
                WHERE date(created_at) = date('now')
                "#,
                [],
                |row| {
                    Ok(CoinOpBudgetReport {
                        spent_mojos: row.get(0)?,
                        executed_ops: row.get(1)?,
                        planned_ops: row.get(2)?,
                        skipped_ops: row.get(3)?,
                        fee_budget_skipped_ops: row.get(4)?,
                    })
                },
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to query coin-op budget report: {err}"))
            })
    }
}
