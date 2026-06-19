use std::path::{Path, PathBuf};

use serde_json::json;
use tracing::Level;

use crate::error::{SignerError, SignerResult};
use crate::operator_log::{operator_audit, AuditDurability, EmitMode, LogContext, CONFIG_RELOADED};
use crate::storage::SqliteStore;

const RELOAD_MARKER_FILE: &str = "reload_request.json";
const RELOAD_DONE_FILE: &str = "reload_done.json";

#[must_use]
pub fn reload_marker_path(state_dir: &Path) -> PathBuf {
    state_dir.join(RELOAD_MARKER_FILE)
}

#[must_use]
pub fn reload_done_marker_path(state_dir: &Path) -> PathBuf {
    state_dir.join(RELOAD_DONE_FILE)
}

#[must_use]
pub fn reload_marker_present(state_dir: &Path) -> bool {
    reload_marker_path(state_dir).is_file()
}

fn cleanup_reload_done_marker(state_dir: &Path) {
    let done = reload_done_marker_path(state_dir);
    if done.is_file() {
        if let Err(err) = std::fs::remove_file(&done) {
            tracing::warn!(
                marker = %done.display(),
                error = %err,
                "failed to remove processed reload marker"
            );
        }
    }
}

/// Move `reload_request.json` to `reload_done.json` after audit succeeds.
fn finalize_reload_marker(state_dir: &Path) {
    let request = reload_marker_path(state_dir);
    let done = reload_done_marker_path(state_dir);
    if std::fs::rename(&request, &done).is_ok() {
        return;
    }
    if remove_reload_marker(state_dir).is_ok() {
        return;
    }
    tracing::warn!(
        request = %request.display(),
        "config reload recorded but marker could not be finalized"
    );
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
    operator_audit(
        Some(store),
        LogContext::CONFIG,
        EmitMode::dual(Level::INFO, "config reloaded"),
        CONFIG_RELOADED,
        &payload,
        None,
        AuditDurability::Required,
    )
}

/// Best-effort reload marker handling for the daemon loop.
pub fn handle_reload_marker_if_present(state_dir: &Path, db_path: &Path) {
    cleanup_reload_done_marker(state_dir);

    if !reload_marker_present(state_dir) {
        return;
    }
    let Ok(store) = SqliteStore::open(db_path) else {
        tracing::warn!(
            db_path = %db_path.display(),
            "config reload marker present but state DB open failed; will retry next cycle"
        );
        return;
    };
    if record_config_reloaded(&store, "reload_marker").is_err() {
        tracing::warn!(
            "config reload marker present but audit insert failed; will retry next cycle"
        );
        return;
    }
    finalize_reload_marker(state_dir);
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
    fn remove_reload_marker_deletes_request_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!reload_marker_present(dir.path()));
        std::fs::write(reload_marker_path(dir.path()), b"{}").expect("write marker");
        assert!(reload_marker_present(dir.path()));
        remove_reload_marker(dir.path()).expect("remove");
        assert!(!reload_marker_present(dir.path()));
    }

    #[test]
    fn handle_reload_marker_renames_request_to_done() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        std::fs::write(reload_marker_path(dir.path()), b"{}").expect("write marker");
        handle_reload_marker_if_present(dir.path(), &db_path);
        assert!(!reload_marker_present(dir.path()));
        assert!(reload_done_marker_path(dir.path()).is_file());
        let store = SqliteStore::open(&db_path).expect("open");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn handle_reload_marker_keeps_request_when_db_open_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(reload_marker_path(dir.path()), b"{}").expect("write marker");
        let blocking = dir.path().join("blocking_file");
        std::fs::write(&blocking, b"x").expect("write blocking file");
        let bad_db = blocking.join("greenfloor.sqlite");
        handle_reload_marker_if_present(dir.path(), &bad_db);
        assert!(reload_marker_present(dir.path()));
    }

    #[test]
    fn handle_reload_marker_does_not_duplicate_audit_when_only_done_marker_remains() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        std::fs::write(reload_done_marker_path(dir.path()), b"{}").expect("write done");
        let store = SqliteStore::open(&db_path).expect("open");
        record_config_reloaded(&store, "reload_marker").expect("seed audit");
        handle_reload_marker_if_present(dir.path(), &db_path);
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 10)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert!(!reload_done_marker_path(dir.path()).is_file());
    }
}
