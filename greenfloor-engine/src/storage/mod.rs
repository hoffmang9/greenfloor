//! `SQLite` persistence for the Rust engine.
//!
//! The canonical schema for `GreenFloor` state lives here. Rust integration tests in
//! `greenfloor-engine/tests/` assert against the same schema via `SqliteStore`.

mod persist;
mod schema;
mod sqlite;

pub use persist::{persist_offer_post_records, upsert_offer_post_record};
pub use sqlite::{
    resolve_state_db_path, state_db_path_for_home, AuditEventRow, CoinOpBudgetReport,
    CoinOpLedgerEntry, OfferPostPersistRecord, OfferReservationAcquireOutcome,
    OfferReservationLeaseRequest, OfferReservationLeaseRow, OfferStateDetailRow, OfferStateListRow,
    SqliteStore, StoredAlertState, TxSignalStateRow,
};
