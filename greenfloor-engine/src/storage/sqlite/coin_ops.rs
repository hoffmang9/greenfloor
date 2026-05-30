use rusqlite::params;

use crate::error::{SignerError, SignerResult};

use super::{utcnow_iso, SqliteStore};

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

    pub fn add_coin_op_ledger_entry(
        &self,
        market_id: &str,
        op_type: &str,
        op_count: i64,
        fee_mojos: i64,
        status: &str,
        reason: &str,
        operation_id: Option<&str>,
    ) -> SignerResult<()> {
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
}
