//! `SQLite` persistence for the Rust engine.
//!
//! The canonical schema for `GreenFloor` state lives here. Rust integration tests in
//! `greenfloor-engine/tests/` assert against the same schema via `SqliteStore`.

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

pub use crate::offer::types::PresplitCancelFields;
pub use persist::{persist_offer_post_records, upsert_offer_post_record};
pub use sqlite::{
    resolve_state_db_path, state_db_path_for_home, AuditEventRow, CoinOpBudgetReport,
    CoinOpLedgerEntry, OfferPostPersistRecord, OfferReservationAcquireOutcome,
    OfferReservationLeaseRequest, OfferReservationLeaseRow, OfferReservationRejectReason,
    OfferStateDetailRow, OfferStateListRow, SqliteStore, StoredAlertState, TxSignalStateRow,
};
