//! `SQLite` persistence for the Rust engine.
//!
//! The canonical schema for `GreenFloor` state lives here. Rust integration tests in
//! `greenfloor-engine/tests/sqlite_*` exercise persistence behavior.
//!
//! `storage/sqlite/` and `storage/test_support.rs` are excluded from llvm-cov reports and
//! diff-cover (see `.llvm-cov.toml`, ADR 0016).

mod audit_retention;
mod persist;
mod schema;
mod sqlite;

pub use audit_retention::{
    audit_prune_interval_seconds, audit_retention_cutoff, is_preserved_audit_row,
    maybe_prune_stale_audit_events, preserve_predicate_sql, PruneAuditEventsOptions,
    PruneAuditEventsReport, DEFAULT_AUDIT_PRUNE_BATCH_SIZE, DEFAULT_AUDIT_PRUNE_INTERVAL_SECONDS,
    DEFAULT_AUDIT_RETENTION_DAYS,
};

pub use crate::offer::types::OfferCancelFields;
pub use persist::{
    persist_offer_post_records, upsert_offer_post_record, write_offer_post_record_in_txn,
};
#[doc(hidden)]
pub mod test_support;
pub use sqlite::{
    resolve_state_db_path, state_db_path_for_home, AuditEventRow, CoinOpBudgetReport,
    CoinOpLedgerEntry, CycleWriteStore, OfferCancelWrite, OfferPostPersistRecord,
    OfferReservationAcquireOutcome, OfferReservationLeaseRequest, OfferReservationLeaseRow,
    OfferReservationRejectReason, OfferStateDetailRow, OfferStateListRow, SqliteStore,
    StoredAlertState, TxSignalIngress, TxSignalStateRow, WatchHitRow, WatchMatchKind,
};

#[cfg(test)]
pub use sqlite::{
    lock_shared_store_for_test, reset_sqlite_open_calls_for_test, sqlite_open_calls_for_test,
};
