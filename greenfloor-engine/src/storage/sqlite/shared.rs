use std::sync::{Arc, Mutex, MutexGuard};

use super::SqliteStore;
use crate::error::{SignerError, SignerResult};

pub type SharedSqliteStore = Arc<Mutex<SqliteStore>>;

#[must_use]
pub fn shared_sqlite_store(store: SqliteStore) -> SharedSqliteStore {
    Arc::new(Mutex::new(store))
}

/// Lock the shared cycle store for one synchronous read/write section.
///
/// # Errors
///
/// Returns an error when the mutex is poisoned.
pub fn lock_sqlite_store(store: &SharedSqliteStore) -> SignerResult<MutexGuard<'_, SqliteStore>> {
    store
        .lock()
        .map_err(|err| SignerError::Other(format!("sqlite store lock poisoned: {err}")))
}

/// Run one synchronous section against the shared cycle store.
///
/// # Errors
///
/// Returns an error when the mutex is poisoned or `f` fails.
pub fn with_sqlite_store<T, F>(store: &SharedSqliteStore, f: F) -> SignerResult<T>
where
    F: FnOnce(&SqliteStore) -> SignerResult<T>,
{
    let guard = lock_sqlite_store(store)?;
    f(&guard)
}

/// Hold the cycle-store lock across one async section (sequential daemon phases only).
///
/// Do not use while parallel offer-dispatch workers may need the same store.
#[macro_export]
macro_rules! with_locked_store {
    ($store:expr, |$guard:ident| $body:expr) => {{
        #[allow(clippy::await_holding_lock)]
        async {
            let $guard = $crate::storage::lock_sqlite_store($store)?;
            $body.await
        }
        .await
    }};
}
