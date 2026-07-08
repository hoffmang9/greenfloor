use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};
use tracing::Level;

use crate::daemon::coinset_ws::{CoinsetProcessContext, InventoryP2Index};
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{LogContext, CONFIG_RELOADED};
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

fn reload_id_from_marker(path: &Path) -> SignerResult<String> {
    let content = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!(
            "failed to read reload marker {}: {err}",
            path.display()
        ))
    })?;
    if let Ok(payload) = serde_json::from_str::<Value>(&content) {
        if let Some(reload_id) = payload
            .get("reload_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(reload_id.to_string());
        }
    }
    let metadata = std::fs::metadata(path).map_err(|err| {
        SignerError::Other(format!(
            "failed to stat reload marker {}: {err}",
            path.display()
        ))
    })?;
    let modified_secs = metadata
        .modified()
        .ok()
        .and_then(|mtime| mtime.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs());
    Ok(format!("legacy-{modified_secs}-{}", metadata.len()))
}

fn config_reload_already_recorded(store: &SqliteStore, reload_id: &str) -> SignerResult<bool> {
    store.recent_audit_payload_matches(CONFIG_RELOADED, "reload_id", reload_id, 50)
}

/// Persist and trace a successful config reload.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn record_config_reloaded(
    store: &SqliteStore,
    source: &str,
    reload_id: &str,
) -> SignerResult<()> {
    let payload = json!({ "source": source, "reload_id": reload_id });
    LogContext::CONFIG.dual_audit(
        store,
        Level::INFO,
        "config reloaded",
        CONFIG_RELOADED,
        &payload,
        None,
    )
}

fn refresh_inventory_p2s_after_reload(
    coinset: &CoinsetProcessContext,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) {
    match InventoryP2Index::from_markets(markets_path, testnet_markets_path) {
        Ok(index) => {
            let p2_count = index.p2s().len();
            coinset.replace_inventory_p2s(index);
            coinset.request_ws_reconnect();
            tracing::info!(
                p2_count,
                markets_path = %markets_path.display(),
                "refreshed inventory p2 index after config reload; websocket reconnect requested"
            );
        }
        Err(err) => {
            tracing::warn!(
                markets_path = %markets_path.display(),
                error = %err,
                "config reload recorded but inventory p2 rebuild failed; keeping prior filters"
            );
        }
    }
}

/// Best-effort reload marker handling for the daemon loop.
///
/// When `coinset` and markets paths are provided, rebuilds the inventory p2 index
/// and asks the Coinset WS loop to reconnect with the new filters.
pub fn handle_reload_marker_if_present(
    state_dir: &Path,
    db_path: &Path,
    coinset: Option<&Arc<CoinsetProcessContext>>,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) {
    let marker = reload_marker_path(state_dir);
    if !marker.is_file() {
        return;
    }
    let reload_id = match reload_id_from_marker(&marker) {
        Ok(reload_id) => reload_id,
        Err(err) => {
            tracing::warn!(
                marker = %marker.display(),
                error = %err,
                "config reload marker unreadable; will retry next cycle"
            );
            return;
        }
    };
    let Ok(store) = SqliteStore::open(db_path) else {
        tracing::warn!(
            db_path = %db_path.display(),
            "config reload marker present but state DB open failed; will retry next cycle"
        );
        return;
    };
    match config_reload_already_recorded(&store, &reload_id) {
        Ok(true) => {
            if let Err(err) = remove_reload_marker(state_dir) {
                tracing::warn!(
                    marker = %marker.display(),
                    error = %err,
                    "config reload already recorded but marker removal failed"
                );
            }
            return;
        }
        Ok(false) => {}
        Err(err) => {
            tracing::warn!(
                reload_id = reload_id.as_str(),
                error = %err,
                "config reload marker present but audit lookup failed; will retry next cycle"
            );
            return;
        }
    }
    if record_config_reloaded(&store, "reload_marker", &reload_id).is_err() {
        tracing::warn!(
            reload_id = reload_id.as_str(),
            "config reload marker present but audit insert failed; will retry next cycle"
        );
        return;
    }
    if let Some(coinset) = coinset {
        refresh_inventory_p2s_after_reload(coinset, markets_path, testnet_markets_path);
    }
    if let Err(err) = remove_reload_marker(state_dir) {
        tracing::warn!(
            marker = %marker.display(),
            error = %err,
            "config reload recorded but marker removal failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_config_reloaded_persists_source_and_reload_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        record_config_reloaded(&store, "reload_marker", "reload-1").expect("reload");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].payload.get("source").and_then(|v| v.as_str()),
            Some("reload_marker")
        );
        assert_eq!(
            events[0].payload.get("reload_id").and_then(|v| v.as_str()),
            Some("reload-1")
        );
    }

    #[test]
    fn remove_reload_marker_deletes_request_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!reload_marker_present(dir.path()));
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        assert!(reload_marker_present(dir.path()));
        remove_reload_marker(dir.path()).expect("remove");
        assert!(!reload_marker_present(dir.path()));
    }

    fn call_reload(state_dir: &Path, db_path: &Path) {
        handle_reload_marker_if_present(state_dir, db_path, None, Path::new("."), None);
    }

    #[test]
    fn handle_reload_marker_records_audit_and_removes_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        call_reload(dir.path(), &db_path);
        assert!(!reload_marker_present(dir.path()));
        let store = SqliteStore::open(&db_path).expect("open");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn handle_reload_marker_keeps_marker_when_db_open_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        let blocking = dir.path().join("blocking_file");
        std::fs::write(&blocking, b"x").expect("write blocking file");
        let bad_db = blocking.join("greenfloor.sqlite");
        call_reload(dir.path(), &bad_db);
        assert!(reload_marker_present(dir.path()));
    }

    #[test]
    fn handle_reload_marker_records_single_audit_across_cycles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        call_reload(dir.path(), &db_path);
        call_reload(dir.path(), &db_path);
        let store = SqliteStore::open(&db_path).expect("open");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 10)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert!(!reload_marker_present(dir.path()));
    }

    #[test]
    fn handle_reload_marker_skips_reaudit_when_reload_id_already_recorded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let store = SqliteStore::open(&db_path).expect("open");
        record_config_reloaded(&store, "reload_marker", "reload-1").expect("seed audit");
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        call_reload(dir.path(), &db_path);
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 10)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert!(!reload_marker_present(dir.path()));
    }

    #[test]
    fn handle_reload_marker_refreshes_inventory_p2_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let coinset = CoinsetProcessContext::empty();
        assert!(coinset.inventory_p2s().p2s().is_empty());
        let p2 = "ab".repeat(32);
        let mut markets_by_p2 = std::collections::HashMap::new();
        markets_by_p2.insert(p2.clone(), vec!["m1".to_string()]);
        // Seed via replace path used by reload helper after a successful from_markets.
        // Exercise replace + reconnect flag directly (from_markets needs real YAML).
        coinset.replace_inventory_p2s(InventoryP2Index::from_markets_by_p2(markets_by_p2));
        coinset.request_ws_reconnect();
        assert_eq!(coinset.inventory_p2s().p2s(), std::slice::from_ref(&p2));
        assert!(coinset.take_ws_reconnect_requested());
        assert!(!coinset.take_ws_reconnect_requested());

        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-p2"}"#,
        )
        .expect("write marker");
        // Missing markets path: audit still records; rebuild warns and keeps prior index.
        handle_reload_marker_if_present(
            dir.path(),
            &db_path,
            Some(&coinset),
            &dir.path().join("missing-markets.yaml"),
            None,
        );
        assert!(!reload_marker_present(dir.path()));
        assert_eq!(coinset.inventory_p2s().p2s(), std::slice::from_ref(&p2));
        assert!(!coinset.take_ws_reconnect_requested());
    }

    #[test]
    fn reload_id_from_legacy_marker_is_stable_for_same_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = reload_marker_path(dir.path());
        std::fs::write(&marker, b"{}").expect("write marker");
        let first = reload_id_from_marker(&marker).expect("reload id");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let second = reload_id_from_marker(&marker).expect("reload id");
        assert_eq!(first, second);
    }
}
