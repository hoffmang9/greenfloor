use std::path::Path;

use greenfloor_engine::storage::SqliteStore;

pub fn open_store(path: &Path) -> SqliteStore {
    SqliteStore::open(path).expect("open sqlite store")
}
