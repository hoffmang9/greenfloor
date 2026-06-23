use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use super::SqliteStore;
use crate::error::{SignerError, SignerResult};

/// One shared cycle sqlite connection guarded for sequential phases and brief parallel writes.
#[derive(Clone, Debug)]
pub struct CycleWriteStore(Arc<Mutex<SqliteStore>>);

impl CycleWriteStore {
    /// Open the cycle DB and wrap it for multi-threaded daemon use.
    ///
    /// # Errors
    ///
    /// Returns an error when [`SqliteStore::open`] fails.
    pub fn open(db_path: &Path) -> SignerResult<Self> {
        Ok(Self(Arc::new(Mutex::new(SqliteStore::open(db_path)?))))
    }

    #[must_use]
    pub fn from_sqlite(store: SqliteStore) -> Self {
        Self(Arc::new(Mutex::new(store)))
    }

    /// Lock the shared cycle store for one read/write section.
    ///
    /// # Errors
    ///
    /// Returns an error when the mutex is poisoned.
    pub fn lock(&self) -> SignerResult<MutexGuard<'_, SqliteStore>> {
        self.0
            .lock()
            .map_err(|err| SignerError::Other(format!("sqlite store lock poisoned: {err}")))
    }

    /// Run one synchronous section against the shared cycle store.
    ///
    /// # Errors
    ///
    /// Returns an error when the mutex is poisoned or `f` fails.
    pub fn sync<T, F>(&self, f: F) -> SignerResult<T>
    where
        F: FnOnce(&SqliteStore) -> SignerResult<T>,
    {
        let guard = self.lock()?;
        f(&guard)
    }
}

/// Hold a [`CycleWriteStore`] lock across one async daemon phase (sequential only).
///
/// Do not use while parallel offer-dispatch workers may need the same store.
#[macro_export]
macro_rules! cycle_locked {
    ($store:expr, |$guard:ident| $body:expr) => {{
        #[allow(clippy::await_holding_lock)]
        async {
            let $guard = ($store).lock()?;
            $body.await
        }
        .await
    }};
}

/// Test helper: lock a shared cycle store and panic on poison (fixture setup only).
#[cfg(test)]
#[allow(clippy::missing_panics_doc)] // test helper: panics on fixture setup failure
pub fn lock_shared_store_for_test(store: &CycleWriteStore) -> MutexGuard<'_, SqliteStore> {
    store.0.lock().expect("lock")
}
