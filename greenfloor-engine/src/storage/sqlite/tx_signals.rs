use std::collections::HashMap;

use rusqlite::params;

use crate::error::{SignerError, SignerResult};

use super::{utcnow_iso, SqliteStore, TxSignalStateRow};

impl SqliteStore {
    pub fn get_tx_signal_state(
        &self,
        tx_ids: &[String],
    ) -> SignerResult<HashMap<String, TxSignalStateRow>> {
        let mut unique: Vec<String> = Vec::new();
        for tx_id in tx_ids {
            let normalized = tx_id.trim();
            if normalized.is_empty() {
                continue;
            }
            if !unique.iter().any(|existing| existing == normalized) {
                unique.push(normalized.to_string());
            }
        }
        if unique.is_empty() {
            return Ok(HashMap::default());
        }
        let placeholders = unique
            .iter()
            .enumerate()
            .map(|(index, _)| format!("?{}", index + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r"
            SELECT tx_id, mempool_observed_at, tx_block_confirmed_at
            FROM tx_signal_state
            WHERE tx_id IN ({placeholders})
            "
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|err| {
            SignerError::Other(format!("failed to prepare tx_signal query: {err}"))
        })?;
        let params: Vec<&dyn rusqlite::ToSql> = unique
            .iter()
            .map(|value| value as &dyn rusqlite::ToSql)
            .collect();
        let mut rows = stmt
            .query(params.as_slice())
            .map_err(|err| SignerError::Other(format!("failed to query tx_signal_state: {err}")))?;
        let mut out = HashMap::default();
        while let Some(row) = rows
            .next()
            .map_err(|err| SignerError::Other(format!("failed to read tx_signal row: {err}")))?
        {
            let tx_id: String = row
                .get(0)
                .map_err(|err| SignerError::Other(format!("failed to read tx_id: {err}")))?;
            out.insert(
                tx_id,
                TxSignalStateRow {
                    mempool_observed_at: row.get(1).ok(),
                    tx_block_confirmed_at: row.get(2).ok(),
                },
            );
        }
        Ok(out)
    }

    pub fn observe_mempool_tx_ids(&self, tx_ids: &[String]) -> SignerResult<u64> {
        if tx_ids.is_empty() {
            return Ok(0);
        }
        let mut inserted = 0_u64;
        let now = utcnow_iso();
        for tx_id in tx_ids {
            let normalized = tx_id.trim();
            if normalized.is_empty() {
                continue;
            }
            let changed = self
                .conn
                .execute(
                    r"
                    INSERT OR IGNORE INTO tx_signal_state (tx_id, mempool_observed_at, tx_block_confirmed_at)
                    VALUES (?1, ?2, NULL)
                    ",
                    params![normalized, now],
                )
                .map_err(|err| {
                    SignerError::Other(format!("failed to observe mempool tx id: {err}"))
                })?;
            inserted += u64::try_from(changed).unwrap_or(0);
        }
        Ok(inserted)
    }

    pub fn confirm_tx_ids(&self, tx_ids: &[String]) -> SignerResult<u64> {
        if tx_ids.is_empty() {
            return Ok(0);
        }
        let now = utcnow_iso();
        let mut updated = 0_u64;
        for tx_id in tx_ids {
            let normalized = tx_id.trim();
            if normalized.is_empty() {
                continue;
            }
            let changed = self
                .conn
                .execute(
                    r"
                    UPDATE tx_signal_state
                    SET tx_block_confirmed_at = COALESCE(tx_block_confirmed_at, ?1)
                    WHERE tx_id = ?2
                    ",
                    params![now, normalized],
                )
                .map_err(|err| SignerError::Other(format!("failed to confirm tx id: {err}")))?;
            updated += u64::try_from(changed).unwrap_or(0);
        }
        Ok(updated)
    }
}
