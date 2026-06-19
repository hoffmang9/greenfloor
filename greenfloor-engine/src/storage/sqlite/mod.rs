//! SQLite store: connection lifecycle and shared row types.

mod alerts;
mod audit;
mod coin_ops;
mod offers;
mod pricing;
mod reservations;
mod tx_signals;

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;

use crate::error::{SignerError, SignerResult};

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
}

pub use coin_ops::{CoinOpBudgetReport, CoinOpLedgerEntry};
pub use reservations::{OfferReservationLeaseRequest, OfferReservationLeaseRow};

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

#[must_use]
pub fn state_db_path_for_home(home_dir: &Path) -> PathBuf {
    home_dir.join("db").join("greenfloor.sqlite")
}

/// Resolve SQLite state DB path (explicit override or default under program home).
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

impl SqliteStore {
    /// Open.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn open(db_path: &Path) -> SignerResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                SignerError::Other(format!(
                    "failed to create sqlite parent dir {}: {err}",
                    parent.display()
                ))
            })?;
        }
        let conn = Connection::open(db_path).map_err(|err| {
            SignerError::Other(format!(
                "failed to open sqlite db {}: {err}",
                db_path.display()
            ))
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
        Ok(Self { conn })
    }
}

pub(crate) fn utcnow_iso() -> String {
    Utc::now().to_rfc3339()
}
