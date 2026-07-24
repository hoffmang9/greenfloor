use std::collections::HashMap;

use rusqlite::params;

use crate::error::{SignerError, SignerResult};
use crate::hex::{canonical_tx_id, extend_tx_id_lookup_candidates, tx_id_lookup_candidates};

use super::{
    in_placeholders, query_mapped, sqlite_rows_changed, utcnow_iso, SqliteStore, TxSignalStateRow,
};

/// How to ingest tx ids into `tx_signal_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxSignalIngress {
    /// First-seen or refresh mempool observation only.
    Mempool,
    /// Observe (if needed) then mark block-confirmed.
    Confirmed,
}

impl SqliteStore {
    /// Ingest tx ids into `tx_signal_state` (canonical observe / observe+confirm).
    ///
    /// `Confirmed` always observes first so a first-seen confirmed frame still seeds a row
    /// (`confirm_tx_ids` only updates existing rows).
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn ingest_tx_signals(&self, tx_ids: &[String], kind: TxSignalIngress) -> SignerResult<u64> {
        match kind {
            TxSignalIngress::Mempool => self.observe_mempool_tx_ids(tx_ids),
            TxSignalIngress::Confirmed => {
                self.observe_mempool_tx_ids(tx_ids)?;
                self.confirm_tx_ids(tx_ids)
            }
        }
    }

    /// Get tx signal state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn get_tx_signal_state(
        &self,
        tx_ids: &[String],
    ) -> SignerResult<HashMap<String, TxSignalStateRow>> {
        let mut unique: Vec<String> = Vec::new();
        for tx_id in tx_ids {
            extend_tx_id_lookup_candidates(&mut unique, tx_id);
        }
        if unique.is_empty() {
            return Ok(HashMap::default());
        }
        let sql = format!(
            r"
            SELECT tx_id, mempool_observed_at, tx_block_confirmed_at
            FROM tx_signal_state
            WHERE tx_id IN ({})
            ",
            in_placeholders(unique.len())
        );
        let rows: Vec<(String, Option<String>, Option<String>)> = query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(unique.iter()),
            "tx_signal_state",
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let mut out = HashMap::default();
        for (tx_id, mempool_observed_at, tx_block_confirmed_at) in rows {
            let Some(key) = canonical_tx_id(&tx_id) else {
                continue;
            };
            out.insert(
                key,
                TxSignalStateRow {
                    mempool_observed_at,
                    tx_block_confirmed_at,
                },
            );
        }
        Ok(out)
    }

    /// Observe mempool tx ids.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn observe_mempool_tx_ids(&self, tx_ids: &[String]) -> SignerResult<u64> {
        if tx_ids.is_empty() {
            return Ok(0);
        }
        let mut inserted = 0_u64;
        let now = utcnow_iso();
        for tx_id in tx_ids {
            let Some(normalized) = canonical_tx_id(tx_id) else {
                continue;
            };
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
            inserted += sqlite_rows_changed(changed)?;
        }
        Ok(inserted)
    }

    /// Confirm tx ids.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn confirm_tx_ids(&self, tx_ids: &[String]) -> SignerResult<u64> {
        if tx_ids.is_empty() {
            return Ok(0);
        }
        let now = utcnow_iso();
        let mut updated = 0_u64;
        for tx_id in tx_ids {
            let Some(normalized) = canonical_tx_id(tx_id) else {
                continue;
            };
            updated += self.confirm_one_tx_id(&now, &normalized)?;
        }
        Ok(updated)
    }

    fn confirm_one_tx_id(&self, now: &str, canonical: &str) -> SignerResult<u64> {
        for candidate in tx_id_lookup_candidates(canonical) {
            let changed = self
                .conn
                .execute(
                    r"
                    UPDATE tx_signal_state
                    SET tx_block_confirmed_at = COALESCE(tx_block_confirmed_at, ?1)
                    WHERE tx_id = ?2
                    ",
                    params![now, candidate],
                )
                .map_err(|err| SignerError::Other(format!("failed to confirm tx id: {err}")))?;
            let rows = sqlite_rows_changed(changed)?;
            if rows > 0 {
                return Ok(rows);
            }
        }
        Ok(0)
    }
}
