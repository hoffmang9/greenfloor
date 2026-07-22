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

    /// Run `body` inside a savepoint-safe transaction (nests under an outer txn).
    ///
    /// Prefer this when the caller may already hold an `immediate_transaction`
    /// (for example post-batch flush).
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot begin, commit, or when `body` fails.
    pub fn unchecked_transaction_scope<F, T>(&self, label: &str, body: F) -> SignerResult<T>
    where
        F: FnOnce(&Self) -> SignerResult<T>,
    {
        let tx = self.conn.unchecked_transaction().map_err(|err| {
            SignerError::Other(format!("failed to begin {label} transaction: {err}"))
        })?;
        match body(self) {
            Ok(value) => {
                tx.commit().map_err(|err| {
                    SignerError::Other(format!("failed to commit {label} transaction: {err}"))
                })?;
                Ok(value)
            }
            Err(err) => {
                let _ = tx.rollback();
                Err(err)
            }
        }
    }
}
