use thiserror::Error;

/// `SQLite` reservation and lock failures.
#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("reservation contention: {0}")]
    ReservationContention(String),

    #[error("database is locked")]
    DatabaseLocked,

    #[error("failed to open sqlite db {path}: {open_error}")]
    SqliteOpenFailed { path: String, open_error: String },
}

impl PersistenceError {
    #[must_use]
    pub fn is_sqlite_fatal(&self) -> bool {
        matches!(self, Self::SqliteOpenFailed { .. })
    }

    #[must_use]
    pub fn is_parallel_dispatch_transient(&self) -> bool {
        matches!(self, Self::ReservationContention(_) | Self::DatabaseLocked)
    }
}
