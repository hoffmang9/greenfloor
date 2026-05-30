//! SQLite persistence for manager offer posts (shared schema with Python daemon).

mod sqlite;

pub use sqlite::{
    persist_offer_post_records, state_db_path_for_home, OfferPostPersistRecord, SqliteStore,
};
