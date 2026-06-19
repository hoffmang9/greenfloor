use std::path::Path;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

#[must_use]
pub fn consume_reload_marker(state_dir: &Path) -> bool {
    let marker = state_dir.join("reload_request.json");
    if !marker.is_file() {
        return false;
    }
    std::fs::remove_file(marker).is_ok()
}

pub fn record_config_reloaded(store: &SqliteStore) -> SignerResult<()> {
    store.add_audit_event("config_reloaded", &serde_json::json!({}), None)
}
