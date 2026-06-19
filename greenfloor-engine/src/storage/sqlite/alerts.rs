use rusqlite::params;

use crate::error::{SignerError, SignerResult};

use super::{utcnow_iso, SqliteStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAlertState {
    pub market_id: String,
    pub is_low: bool,
    pub last_alert_at: Option<String>,
}

impl SqliteStore {
    /// Get alert state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn get_alert_state(&self, market_id: &str) -> SignerResult<StoredAlertState> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT market_id, is_low, last_alert_at FROM alert_state WHERE market_id = ?1",
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to prepare alert_state query: {err}"))
            })?;
        let mut rows = stmt
            .query(params![market_id])
            .map_err(|err| SignerError::Other(format!("failed to query alert_state: {err}")))?;
        let Some(row) = rows
            .next()
            .map_err(|err| SignerError::Other(format!("failed to read alert_state row: {err}")))?
        else {
            return Ok(StoredAlertState {
                market_id: market_id.to_string(),
                is_low: false,
                last_alert_at: None,
            });
        };
        let is_low: i64 = row
            .get(1)
            .map_err(|err| SignerError::Other(format!("failed to read is_low: {err}")))?;
        Ok(StoredAlertState {
            market_id: row
                .get(0)
                .map_err(|err| SignerError::Other(format!("failed to read market_id: {err}")))?,
            is_low: is_low != 0,
            last_alert_at: row.get(2).ok(),
        })
    }

    /// Upsert alert state.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn upsert_alert_state(&self, state: &StoredAlertState) -> SignerResult<()> {
        self.conn
            .execute(
                r"
                INSERT INTO alert_state (market_id, is_low, last_alert_at, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(market_id) DO UPDATE SET
                  is_low = excluded.is_low,
                  last_alert_at = excluded.last_alert_at,
                  updated_at = excluded.updated_at
                ",
                params![
                    state.market_id,
                    i64::from(state.is_low),
                    state.last_alert_at,
                    utcnow_iso(),
                ],
            )
            .map_err(|err| SignerError::Other(format!("failed to upsert alert_state: {err}")))?;
        Ok(())
    }
}
