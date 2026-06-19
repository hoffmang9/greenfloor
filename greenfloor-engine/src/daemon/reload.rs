use std::path::{Path, PathBuf};

use serde_json::json;
use tracing::Level;

use crate::error::{SignerError, SignerResult};
use crate::operator_log::{audit_and_trace, LogContext, CONFIG_RELOADED};
use crate::storage::SqliteStore;

const RELOAD_MARKER_FILE: &str = "reload_request.json";

#[must_use]
pub fn reload_marker_path(state_dir: &Path) -> PathBuf {
    state_dir.join(RELOAD_MARKER_FILE)
}

#[must_use]
pub fn reload_marker_present(state_dir: &Path) -> bool {
    reload_marker_path(state_dir).is_file()
}

/// Remove the reload marker after config reload is recorded.
///
/// # Errors
///
/// Returns an error when the marker file cannot be removed.
pub fn remove_reload_marker(state_dir: &Path) -> SignerResult<()> {
    let marker = reload_marker_path(state_dir);
    if !marker.is_file() {
        return Ok(());
    }
    std::fs::remove_file(&marker).map_err(|err| {
        SignerError::Other(format!(
            "failed to remove reload marker {}: {err}",
            marker.display()
        ))
    })
}

/// Persist and trace a successful config reload.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
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

    #[test]
    fn remove_reload_marker_deletes_only_after_audit_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!reload_marker_present(dir.path()));
        std::fs::write(reload_marker_path(dir.path()), b"{}").expect("write marker");
        assert!(reload_marker_present(dir.path()));
        remove_reload_marker(dir.path()).expect("remove");
        assert!(!reload_marker_present(dir.path()));
    }
}
