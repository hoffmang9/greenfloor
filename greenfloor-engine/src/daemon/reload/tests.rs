use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;
use crate::coinset::puzzle_hash_hex_for_receive_address;
use crate::hex::normalize_hex_id;
use crate::operator_log::CONFIG_RELOADED;

const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

struct Harness {
    dir: TempDir,
    db_path: PathBuf,
    markets_path: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let markets_path = dir.path().join("markets.yaml");
        std::fs::write(
            &markets_path,
            format!(
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
            ),
        )
        .expect("write markets");
        let db_path = dir.path().join("greenfloor.sqlite");
        Self {
            dir,
            db_path,
            markets_path,
        }
    }

    fn state_dir(&self) -> &Path {
        self.dir.path()
    }

    fn write_marker(&self, reload_id: &str) {
        std::fs::write(
            reload_marker_path(self.state_dir()),
            format!(r#"{{"reload_id":"{reload_id}"}}"#),
        )
        .expect("write marker");
    }

    fn call(&self) {
        handle_reload_marker_if_present(
            self.state_dir(),
            &self.db_path,
            &CoinsetWsShared::empty(),
            &self.markets_path,
            None,
        );
    }

    fn call_with_coinset(&self, coinset: &Arc<CoinsetWsShared>, markets_path: &Path) {
        handle_reload_marker_if_present(
            self.state_dir(),
            &self.db_path,
            coinset,
            markets_path,
            None,
        );
    }

    fn open_store(&self) -> SqliteStore {
        SqliteStore::open(&self.db_path).expect("open")
    }

    fn reload_events(&self, limit: usize) -> Vec<crate::storage::AuditEventRow> {
        self.open_store()
            .list_recent_audit_events(Some(&[CONFIG_RELOADED]), None, limit)
            .expect("events")
    }
}

fn expected_receive_p2() -> String {
    normalize_hex_id(&puzzle_hash_hex_for_receive_address(RECEIVE_ADDRESS).expect("p2"))
}

fn payload_str<'a>(events: &'a [crate::storage::AuditEventRow], key: &str) -> Option<&'a str> {
    events
        .first()
        .and_then(|event| event.payload.get(key))
        .and_then(|value| value.as_str())
}

#[test]
fn record_config_reloaded_persists_source_reload_id_and_rebuild_status() {
    let h = Harness::new();
    let store = h.open_store();
    record_config_reloaded(
        &store,
        "reload_marker",
        "reload-1",
        InventoryP2RebuildStatus::Ok,
    )
    .expect("reload");
    let events = h.reload_events(1);
    assert_eq!(events.len(), 1);
    assert_eq!(payload_str(&events, "source"), Some("reload_marker"));
    assert_eq!(payload_str(&events, "reload_id"), Some("reload-1"));
    assert_eq!(payload_str(&events, "inventory_p2_rebuild"), Some("ok"));
}

#[test]
fn remove_reload_marker_deletes_request_file() {
    let h = Harness::new();
    assert!(!reload_marker_present(h.state_dir()));
    h.write_marker("reload-1");
    assert!(reload_marker_present(h.state_dir()));
    remove_reload_marker(h.state_dir()).expect("remove");
    assert!(!reload_marker_present(h.state_dir()));
}

#[test]
fn handle_reload_marker_records_audit_and_removes_marker() {
    let h = Harness::new();
    h.write_marker("reload-1");
    h.call();
    assert!(!reload_marker_present(h.state_dir()));
    let events = h.reload_events(1);
    assert_eq!(events.len(), 1);
    assert_eq!(payload_str(&events, "inventory_p2_rebuild"), Some("ok"));
}

#[test]
fn handle_reload_marker_keeps_marker_when_db_open_fails() {
    let h = Harness::new();
    h.write_marker("reload-1");
    let blocking = h.state_dir().join("blocking_file");
    std::fs::write(&blocking, b"x").expect("write blocking file");
    handle_reload_marker_if_present(
        h.state_dir(),
        &blocking.join("greenfloor.sqlite"),
        &CoinsetWsShared::empty(),
        &h.markets_path,
        None,
    );
    assert!(reload_marker_present(h.state_dir()));
}

#[test]
fn handle_reload_marker_records_single_audit_across_cycles() {
    let h = Harness::new();
    h.write_marker("reload-1");
    h.call();
    h.call();
    assert_eq!(h.reload_events(10).len(), 1);
    assert!(!reload_marker_present(h.state_dir()));
}

#[test]
fn handle_reload_marker_skips_reaudit_when_reload_id_already_recorded() {
    let h = Harness::new();
    record_config_reloaded(
        &h.open_store(),
        "reload_marker",
        "reload-1",
        InventoryP2RebuildStatus::Ok,
    )
    .expect("seed audit");
    h.write_marker("reload-1");
    h.call();
    assert_eq!(h.reload_events(10).len(), 1);
    assert!(!reload_marker_present(h.state_dir()));
}

#[test]
fn handle_reload_marker_keeps_prior_index_when_rebuild_fails() {
    let h = Harness::new();
    let coinset = CoinsetWsShared::empty();
    let p2 = "ab".repeat(32);
    let mut markets_by_p2 = std::collections::HashMap::new();
    markets_by_p2.insert(p2.clone(), vec!["m1".to_string()]);
    coinset.replace_p2_index(InventoryP2Index::from_markets_by_p2(markets_by_p2));
    assert_eq!(coinset.p2_index().p2s(), std::slice::from_ref(&p2));

    h.write_marker("reload-p2-fail");
    h.call_with_coinset(&coinset, &h.state_dir().join("missing-markets.yaml"));

    assert!(!reload_marker_present(h.state_dir()));
    assert_eq!(coinset.p2_index().p2s(), std::slice::from_ref(&p2));
    assert!(!coinset.take_reconnect_requested());
    assert_eq!(
        payload_str(&h.reload_events(1), "inventory_p2_rebuild"),
        Some("failed")
    );
}

#[test]
fn handle_reload_marker_rebuilds_inventory_p2_index_and_requests_reconnect() {
    let h = Harness::new();
    let coinset = CoinsetWsShared::empty();
    assert!(coinset.p2_index().p2s().is_empty());

    h.write_marker("reload-p2-ok");
    h.call_with_coinset(&coinset, &h.markets_path);

    let expected = expected_receive_p2();
    assert_eq!(coinset.p2_index().p2s(), std::slice::from_ref(&expected));
    assert!(coinset.take_reconnect_requested());
    assert!(!reload_marker_present(h.state_dir()));
    assert_eq!(
        payload_str(&h.reload_events(1), "inventory_p2_rebuild"),
        Some("ok")
    );
}

#[test]
fn reload_id_from_legacy_marker_is_stable_for_same_file() {
    let h = Harness::new();
    let marker = reload_marker_path(h.state_dir());
    std::fs::write(&marker, b"{}").expect("write marker");
    let first = reload_id_from_marker(&marker).expect("reload id");
    std::thread::sleep(std::time::Duration::from_millis(10));
    let second = reload_id_from_marker(&marker).expect("reload id");
    assert_eq!(first, second);
}
