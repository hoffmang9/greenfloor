use crate::error::{SignerError, SignerResult};

use super::SqliteStore;

impl SqliteStore {
    /// Run `body` inside `BEGIN IMMEDIATE` / `COMMIT`, rolling back on error.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot begin, commit, or when `body` fails.
    pub fn immediate_transaction<F, T>(&self, label: &str, body: F) -> SignerResult<T>
    where
        F: FnOnce(&Self) -> SignerResult<T>,
    {
        self.conn.execute("BEGIN IMMEDIATE", []).map_err(|err| {
            SignerError::Other(format!("failed to begin {label} transaction: {err}"))
        })?;
        match body(self) {
            Ok(value) => {
                self.conn.execute("COMMIT", []).map_err(|err| {
                    SignerError::Other(format!("failed to commit {label} transaction: {err}"))
                })?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn.execute("ROLLBACK", []);
                Err(err)
            }
        }
    }
}
