//! `SQLite` store: connection lifecycle and shared row types.

mod alerts;
mod audit;
mod coin_ops;
mod migrations;
mod offer_cancel;
mod offer_coin_watches;
mod offer_presplit_makers;
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
use rusqlite::{Connection, Row};

use crate::error::{PersistenceError, SignerError, SignerResult};
use crate::offer::types::{OfferCancelFields, OfferExecutionMode};

use super::schema::schema_sql;

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
    pub cancel_fields: OfferCancelFields,
    pub execution_mode: Option<OfferExecutionMode>,
    /// Maker coin ids to watch on Coinset WS (from create/select or offer decode).
    pub watched_coin_ids: Vec<String>,
    /// Maker puzzle hashes (p2) to watch on Coinset WS when known at post time.
    pub watched_p2s: Vec<String>,
    /// Soft listing expiry (unix seconds) for reconcile-driven expire/repost.
    pub listing_expires_at: Option<u64>,
    /// Offer nonce hex (presplit reuse after soft listing expiry).
    pub offer_nonce: Option<String>,
}

pub use coin_ops::{CoinOpBudgetReport, CoinOpLedgerEntry};
pub use offer_cancel::{OfferCancelWrite, OfferListingWrite};
pub use offer_coin_watches::{WatchHitRow, WatchMatchKind};
pub use offer_presplit_makers::{
    OfferListingFields, ReusablePresplitMakerRow, MAKER_CLAIM_RENEW_INTERVAL_SECONDS,
    MAKER_CLAIM_STALE_SECONDS,
};
pub use reservations::{
    OfferReservationAcquireOutcome, OfferReservationLeaseRequest, OfferReservationLeaseRow,
    OfferReservationRejectReason,
};

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn db_err(context: &str, err: rusqlite::Error) -> SignerError {
    if is_sqlite_lock_error(&err) {
        return PersistenceError::DatabaseLocked.into();
    }
    SignerError::Other(format!("{context}: {err}"))
}

fn is_sqlite_lock_error(err: &rusqlite::Error) -> bool {
    matches!(
        err.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    )
}

/// Numbered `?1, ?2, …` placeholders for `IN (...)` clauses.
pub(crate) fn in_placeholders(count: usize) -> String {
    (1..=count)
        .map(|idx| format!("?{idx}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Prepare + `query_map` + collect with consistent error context.
pub(crate) fn query_mapped<T, P, F>(
    conn: &Connection,
    sql: &str,
    params: P,
    context: &str,
    map_row: F,
) -> SignerResult<Vec<T>>
where
    P: rusqlite::Params,
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| db_err(&format!("prepare {context}"), err))?;
    let rows = stmt
        .query_map(params, map_row)
        .map_err(|err| db_err(&format!("query {context}"), err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_err(&format!("read {context}"), err))
}

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

impl OfferStateListRow {
    /// Parse persisted `state` once at the `SQLite` read boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when `state` is not a known reconcile/lifecycle value.
    pub fn reconcile_state(
        &self,
    ) -> Result<crate::cycle::ReconcileState, crate::cycle::ReconcileStateError> {
        crate::cycle::ReconcileState::parse(&self.state)
    }
}

pub use tx_signals::TxSignalIngress;

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
        let conn = Connection::open(db_path).map_err(|err| {
            SignerError::Persistence(PersistenceError::SqliteOpenFailed {
                path: db_path.display().to_string(),
                open_error: err.to_string(),
            })
        })?;
        conn.busy_timeout(Duration::from_secs(30)).map_err(|err| {
            SignerError::Other(format!("failed to set sqlite busy_timeout: {err}"))
        })?;
        conn.execute_batch("PRAGMA busy_timeout = 30000;")
            .map_err(|err| {
                SignerError::Other(format!("failed to set busy_timeout pragma: {err}"))
            })?;
        conn.execute_batch(schema_sql()).map_err(|err| {
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

#[cfg(test)]
mod tests {
    use super::db_err;
    use crate::error::{PersistenceError, SignerError};

    fn sqlite_failure(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None)
    }

    #[test]
    fn db_err_maps_busy_and_locked_to_database_locked() {
        const SQLITE_BUSY: i32 = 5;
        const SQLITE_LOCKED: i32 = 6;
        assert!(matches!(
            db_err("query offers", sqlite_failure(SQLITE_BUSY)),
            SignerError::Persistence(PersistenceError::DatabaseLocked)
        ));
        assert!(matches!(
            db_err("query offers", sqlite_failure(SQLITE_LOCKED)),
            SignerError::Persistence(PersistenceError::DatabaseLocked)
        ));
        let other = db_err("query offers", sqlite_failure(1));
        assert!(!matches!(
            other,
            SignerError::Persistence(PersistenceError::DatabaseLocked)
        ));
        assert!(other.to_string().contains("query offers"));
    }
}
