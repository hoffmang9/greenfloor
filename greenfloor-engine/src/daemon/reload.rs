use std::path::Path;

use serde_json::json;
use tracing::Level;

use crate::error::SignerResult;
use crate::operator_log::{audit_and_trace, LogContext, CONFIG_RELOADED};
use crate::storage::SqliteStore;

#[must_use]
pub fn consume_reload_marker(state_dir: &Path) -> bool {
    let marker = state_dir.join("reload_request.json");
    if !marker.is_file() {
        return false;
    }
    std::fs::remove_file(marker).is_ok()
}

pub fn record_config_reloaded(store: &SqliteStore, source: &str) -> SignerResult<()> {
    let payload = json!({ "source": source });
    audit_and_trace(
        store,
        Level::INFO,
        LogContext::CONFIG,
        CONFIG_RELOADED,
        &payload,
        None,
        "config reloaded",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_config_reloaded_persists_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        record_config_reloaded(&store, "reload_marker").expect("reload");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].payload.get("source").and_then(|v| v.as_str()),
            Some("reload_marker")
        );
    }
}
