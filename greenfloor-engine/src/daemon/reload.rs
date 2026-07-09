use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};
use tracing::Level;

use crate::daemon::coinset_ws::{CoinsetWsShared, InventoryP2Index};
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{LogContext, CONFIG_RELOADED};
use crate::storage::SqliteStore;

const RELOAD_MARKER_FILE: &str = "reload_request.json";

/// Inventory p2 rebuild outcome recorded on `config_reloaded` audit events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryP2RebuildStatus {
    Ok,
    Failed,
}

impl InventoryP2RebuildStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
        }
    }
}

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
    inventory_p2_rebuild: InventoryP2RebuildStatus,
) -> SignerResult<()> {
    let payload = json!({
        "source": source,
        "reload_id": reload_id,
        "inventory_p2_rebuild": inventory_p2_rebuild.as_str(),
    });
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
    coinset: &CoinsetWsShared,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) -> InventoryP2RebuildStatus {
    match InventoryP2Index::from_markets(markets_path, testnet_markets_path) {
        Ok(index) => {
            let p2_count = index.p2s().len();
            coinset.replace_p2_index(index);
            coinset.request_reconnect();
            tracing::info!(
                p2_count,
                markets_path = %markets_path.display(),
                "refreshed inventory p2 index after config reload; websocket reconnect requested"
            );
            InventoryP2RebuildStatus::Ok
        }
        Err(err) => {
            tracing::warn!(
                markets_path = %markets_path.display(),
                error = %err,
                "config reload recorded but inventory p2 rebuild failed; keeping prior filters"
            );
            InventoryP2RebuildStatus::Failed
        }
    }
}

/// Best-effort reload marker handling for the daemon loop.
///
/// Rebuilds the inventory p2 index from markets and asks the Coinset WS loop to
/// reconnect with the new filters. Audit payload includes `inventory_p2_rebuild`
/// (`ok` or `failed`); a failed rebuild keeps prior filters.
pub fn handle_reload_marker_if_present(
    state_dir: &Path,
    db_path: &Path,
    coinset: &Arc<CoinsetWsShared>,
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
    let rebuild_status =
        refresh_inventory_p2s_after_reload(coinset, markets_path, testnet_markets_path);
    if record_config_reloaded(&store, "reload_marker", &reload_id, rebuild_status).is_err() {
        tracing::warn!(
            reload_id = reload_id.as_str(),
            inventory_p2_rebuild = rebuild_status.as_str(),
            "config reload marker present but audit insert failed; will retry next cycle"
        );
        return;
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
    use crate::coinset::puzzle_hash_hex_for_receive_address;
    use crate::hex::normalize_hex_id;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

    fn write_xch_markets(path: &Path) {
        let yaml = format!(
            r#"markets:
  - id: m1
    enabled: true
    base_asset: "xch"
    base_symbol: "XCH"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-1"
    receive_address: "{RECEIVE_ADDRESS}"
    mode: "sell_only"
"#
        );
        std::fs::write(path, yaml).expect("write markets");
    }

    fn expected_receive_p2() -> String {
        normalize_hex_id(&puzzle_hash_hex_for_receive_address(RECEIVE_ADDRESS).expect("p2"))
    }

    #[test]
    fn record_config_reloaded_persists_source_reload_id_and_rebuild_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        record_config_reloaded(
            &store,
            "reload_marker",
            "reload-1",
            InventoryP2RebuildStatus::Ok,
        )
        .expect("reload");
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
        assert_eq!(
            events[0]
                .payload
                .get("inventory_p2_rebuild")
                .and_then(|v| v.as_str()),
            Some("ok")
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

    fn call_reload(state_dir: &Path, db_path: &Path, markets_path: &Path) {
        let coinset = CoinsetWsShared::empty();
        handle_reload_marker_if_present(state_dir, db_path, &coinset, markets_path, None);
    }

    #[test]
    fn handle_reload_marker_records_audit_and_removes_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let markets_path = dir.path().join("markets.yaml");
        write_xch_markets(&markets_path);
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        call_reload(dir.path(), &db_path, &markets_path);
        assert!(!reload_marker_present(dir.path()));
        let store = SqliteStore::open(&db_path).expect("open");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0]
                .payload
                .get("inventory_p2_rebuild")
                .and_then(|v| v.as_str()),
            Some("ok")
        );
    }

    #[test]
    fn handle_reload_marker_keeps_marker_when_db_open_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let markets_path = dir.path().join("markets.yaml");
        write_xch_markets(&markets_path);
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        let blocking = dir.path().join("blocking_file");
        std::fs::write(&blocking, b"x").expect("write blocking file");
        let bad_db = blocking.join("greenfloor.sqlite");
        call_reload(dir.path(), &bad_db, &markets_path);
        assert!(reload_marker_present(dir.path()));
    }

    #[test]
    fn handle_reload_marker_records_single_audit_across_cycles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let markets_path = dir.path().join("markets.yaml");
        write_xch_markets(&markets_path);
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        call_reload(dir.path(), &db_path, &markets_path);
        call_reload(dir.path(), &db_path, &markets_path);
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
        let markets_path = dir.path().join("markets.yaml");
        write_xch_markets(&markets_path);
        let store = SqliteStore::open(&db_path).expect("open");
        record_config_reloaded(
            &store,
            "reload_marker",
            "reload-1",
            InventoryP2RebuildStatus::Ok,
        )
        .expect("seed audit");
        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-1"}"#,
        )
        .expect("write marker");
        call_reload(dir.path(), &db_path, &markets_path);
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 10)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert!(!reload_marker_present(dir.path()));
    }

    #[test]
    fn handle_reload_marker_keeps_prior_index_when_rebuild_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let coinset = CoinsetWsShared::empty();
        let p2 = "ab".repeat(32);
        let mut markets_by_p2 = std::collections::HashMap::new();
        markets_by_p2.insert(p2.clone(), vec!["m1".to_string()]);
        coinset.replace_p2_index(InventoryP2Index::from_markets_by_p2(markets_by_p2));
        assert_eq!(coinset.p2_index().p2s(), std::slice::from_ref(&p2));

        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-p2-fail"}"#,
        )
        .expect("write marker");
        handle_reload_marker_if_present(
            dir.path(),
            &db_path,
            &coinset,
            &dir.path().join("missing-markets.yaml"),
            None,
        );
        assert!(!reload_marker_present(dir.path()));
        assert_eq!(coinset.p2_index().p2s(), std::slice::from_ref(&p2));
        assert!(!coinset.take_reconnect_requested());
        let store = SqliteStore::open(&db_path).expect("open");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 1)
            .expect("events");
        assert_eq!(
            events[0]
                .payload
                .get("inventory_p2_rebuild")
                .and_then(|v| v.as_str()),
            Some("failed")
        );
    }

    #[test]
    fn handle_reload_marker_rebuilds_inventory_p2_index_and_requests_reconnect() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("greenfloor.sqlite");
        let markets_path = dir.path().join("markets.yaml");
        write_xch_markets(&markets_path);
        let coinset = CoinsetWsShared::empty();
        assert!(coinset.p2_index().p2s().is_empty());

        std::fs::write(
            reload_marker_path(dir.path()),
            br#"{"reload_id":"reload-p2-ok"}"#,
        )
        .expect("write marker");
        handle_reload_marker_if_present(dir.path(), &db_path, &coinset, &markets_path, None);

        let expected = expected_receive_p2();
        assert_eq!(coinset.p2_index().p2s(), std::slice::from_ref(&expected));
        assert!(coinset.take_reconnect_requested());
        assert!(!reload_marker_present(dir.path()));
        let store = SqliteStore::open(&db_path).expect("open");
        let events = store
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, 1)
            .expect("events");
        assert_eq!(
            events[0]
                .payload
                .get("inventory_p2_rebuild")
                .and_then(|v| v.as_str()),
            Some("ok")
        );
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
