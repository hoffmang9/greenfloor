use rusqlite::params;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};

use super::{utcnow_iso, SqliteStore};

impl SqliteStore {
    /// Add price policy snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn add_price_policy_snapshot(
        &self,
        market_id: &str,
        payload: &Value,
        source: &str,
    ) -> SignerResult<()> {
        let payload_json = serde_json::to_string(payload).map_err(|err| {
            SignerError::Other(format!(
                "failed to encode price policy snapshot json: {err}"
            ))
        })?;
        self.conn
            .execute(
                r"
                INSERT INTO price_policy_history (market_id, source, payload_json, created_at)
                VALUES (?1, ?2, ?3, ?4)
                ",
                params![market_id, source, payload_json, utcnow_iso()],
            )
            .map_err(|err| {
                SignerError::Other(format!("failed to insert price_policy_history row: {err}"))
            })?;
        Ok(())
    }
}
