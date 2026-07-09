//! `SQLite` store: connection lifecycle and shared row types.

mod alerts;
mod audit;
mod coin_ops;
mod migrations;
mod offer_cancel;
mod offer_coin_watches;
mod offers;
mod pricing;
mod reservations;
mod shared;
mod transaction;
mod tx_signals;

pub use shared::CycleWriteStore;

#[cfg(test)]
pub use shared::lock_shared_store_for_test;

use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;
use rusqlite::Connection;

use crate::error::{SignerError, SignerResult};
use crate::offer::types::{OfferExecutionMode, PresplitCancelFields};

use super::schema::SCHEMA;

#[derive(Debug, Clone)]
pub struct OfferPostPersistRecord {
    pub offer_id: String,
    pub market_id: String,
    pub side: String,
    pub size_base_units: u64,
    pub publish_venue: String,
    pub resolved_base_asset_id: String,
    pub resolved_quote_asset_id: String,
    pub created_extra: serde_json::Value,
    pub cancel_fields: PresplitCancelFields,
    pub execution_mode: Option<OfferExecutionMode>,
    /// Maker coin ids to watch on Coinset WS (from create/select or offer decode).
    pub watched_coin_ids: Vec<String>,
    /// Maker puzzle hashes (p2) to watch on Coinset WS when known at post time.
    pub watched_p2s: Vec<String>,
}

pub use coin_ops::{CoinOpBudgetReport, CoinOpLedgerEntry};
pub use offer_cancel::OfferCancelWrite;
pub use reservations::{
    OfferReservationAcquireOutcome, OfferReservationLeaseRequest, OfferReservationLeaseRow,
    OfferReservationRejectReason,
};

pub(crate) fn sqlite_rows_changed(changed: usize) -> SignerResult<u64> {
    u64::try_from(changed).map_err(|_| {
        SignerError::Other(format!(
            "sqlite rows_changed count {changed} exceeds platform u64::MAX"
        ))
    })
}

pub struct SqliteStore {
    pub(crate) conn: Connection,
}

impl std::fmt::Debug for SqliteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteStore").finish_non_exhaustive()
    }
}

#[must_use]
pub fn state_db_path_for_home(home_dir: &Path) -> PathBuf {
    home_dir.join("db").join("greenfloor.sqlite")
}

/// Resolve `SQLite` state DB path (explicit override or default under program home).
pub fn resolve_state_db_path(home_dir: &Path, explicit_db_path: Option<&str>) -> PathBuf {
    if let Some(path) = explicit_db_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(path);
    }
    state_db_path_for_home(home_dir)
}

#[derive(Debug, Clone)]
pub struct OfferStateListRow {
    pub offer_id: String,
    pub market_id: String,
    pub state: String,
    pub last_seen_status: Option<i64>,
    pub updated_at: String,
    pub cancel_submitted_tx_id: Option<String>,
    pub cancel_submitted_at: Option<String>,
    /// Publish venue at post time (`coinset` / `dexie` / `splash`); `None` for legacy rows.
    pub publish_venue: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OfferStateDetailRow {
    pub offer_id: String,
    pub market_id: String,
    pub state: String,
    pub last_seen_status: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct TxSignalStateRow {
    pub mempool_observed_at: Option<String>,
    pub tx_block_confirmed_at: Option<String>,
}

pub use alerts::StoredAlertState;

pub struct AuditEventRow {
    pub id: i64,
    pub event_type: String,
    pub market_id: Option<String>,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[cfg(test)]
static SQLITE_OPEN_CALLS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub fn reset_sqlite_open_calls_for_test() {
    SQLITE_OPEN_CALLS.store(0, Ordering::SeqCst);
}

#[cfg(test)]
#[must_use]
pub fn sqlite_open_calls_for_test() -> usize {
    SQLITE_OPEN_CALLS.load(Ordering::SeqCst)
}

impl SqliteStore {
    /// Open.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn open(db_path: &Path) -> SignerResult<Self> {
        #[cfg(test)]
        SQLITE_OPEN_CALLS.fetch_add(1, Ordering::SeqCst);
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                SignerError::Other(format!(
                    "failed to create sqlite parent dir {}: {err}",
                    parent.display()
                ))
            })?;
        }
        let conn = Connection::open(db_path).map_err(|err| SignerError::SqliteOpenFailed {
            path: db_path.display().to_string(),
            open_error: err.to_string(),
        })?;
        conn.busy_timeout(Duration::from_secs(30)).map_err(|err| {
            SignerError::Other(format!("failed to set sqlite busy_timeout: {err}"))
        })?;
        conn.execute_batch("PRAGMA busy_timeout = 30000;")
            .map_err(|err| {
                SignerError::Other(format!("failed to set busy_timeout pragma: {err}"))
            })?;
        conn.execute_batch(SCHEMA).map_err(|err| {
            SignerError::Other(format!("failed to initialize sqlite schema: {err}"))
        })?;
        migrations::apply_schema_migrations(&conn)?;
        Ok(Self { conn })
    }

    /// Open and wrap in [`CycleWriteStore`] for multi-threaded cycle use.
    ///
    /// # Errors
    ///
    /// Returns an error when [`Self::open`] fails.
    #[deprecated(note = "use CycleWriteStore::open instead")]
    pub fn open_shared(db_path: &Path) -> SignerResult<CycleWriteStore> {
        CycleWriteStore::open(db_path)
    }
}

pub(crate) fn utcnow_iso() -> String {
    Utc::now().to_rfc3339()
}
