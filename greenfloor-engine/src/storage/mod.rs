//! SQLite persistence for the Rust engine.
//!
//! The canonical schema for GreenFloor state lives here. Python `greenfloor/storage/sqlite.py`
//! remains for the daemon until that path migrates; new manager CLI persistence must use this
//! module only.

mod persist;
mod schema;
mod sqlite;

pub use persist::persist_offer_post_records;
pub use sqlite::{
    state_db_path_for_home, AuditEventRow, OfferPostPersistRecord, OfferStateDetailRow,
    OfferStateListRow, SqliteStore, TxSignalStateRow,
};
