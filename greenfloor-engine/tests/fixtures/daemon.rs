use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use super::json_util::parse_json_output;

#[path = "program.rs"]
mod program_fixture;

pub use program_fixture::{write_minimal_program, MinimalProgramParams};

pub struct DaemonOnceResult {
    pub exit_code: i32,
    pub response: Option<Value>,
}

pub struct DaemonRequestParams<'a> {
    pub program: &'a Path,
    pub markets: &'a Path,
    pub home: &'a Path,
    pub db_path: &'a Path,
    pub coinset_base: &'a str,
    pub poll_coinset_mempool: bool,
    pub test_controls: Value,
}

pub fn run_daemon_once(request: &Value, env: &[(&str, &str)]) -> DaemonOnceResult {
    let dir = tempfile::tempdir().expect("tempdir");
    let request_path = dir.path().join("once_request.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec(request).expect("encode request"),
    )
    .expect("write request json");

    let mut command = Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"));
    command.args([
        "daemon-once",
        "--request-json",
        request_path.to_str().expect("request path"),
        "--json",
    ]);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .output()
        .expect("spawn greenfloor-engine daemon-once");
    let exit_code = output.status.code().unwrap_or(-1);
    let response = if output.stdout.is_empty() {
        None
    } else {
        Some(parse_json_output(&output.stdout))
    };
    DaemonOnceResult {
        exit_code,
        response,
    }
}

pub fn daemon_request(params: DaemonRequestParams<'_>) -> Value {
    let DaemonRequestParams {
        program,
        markets,
        home,
        db_path,
        coinset_base,
        poll_coinset_mempool,
        test_controls,
    } = params;
    json!({
        "program_path": program,
        "markets_path": markets,
        "coinset_base_url": coinset_base,
        "state_dir": home.join("state"),
        "poll_coinset_mempool": poll_coinset_mempool,
        "use_websocket_capture": false,
        "allowed_key_ids": [],
        "dispatch_state": {"cursor": 0, "immediate_requeue_ids": []},
        "test_controls": test_controls,
        "state_db_override": db_path,
    })
}

pub fn write_daemon_program(path: &Path, home_dir: &Path, dexie_api_base: &str) {
    write_minimal_program(
        path,
        MinimalProgramParams {
            home_dir,
            dexie_api_base,
            ..Default::default()
        },
    );
}

pub fn write_markets_one(path: &Path, cancel_policy: bool) {
    let pricing = if cancel_policy {
        "    pricing:\n      cancel_policy_stable_vs_unstable: true\n"
    } else {
        ""
    };
    let yaml = format!(
        r#"markets:
  - id: m1
    enabled: true
    base_asset: "asset1"
    base_symbol: "AS1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
{pricing}    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#
    );
    std::fs::write(path, yaml).expect("write markets yaml");
}

pub fn write_markets_two(path: &Path) {
    let yaml = r#"markets:
  - id: m1
    enabled: true
    base_asset: "asset1"
    base_symbol: "AS1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
  - id: m2
    enabled: true
    base_asset: "asset2"
    base_symbol: "AS2"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#;
    std::fs::write(path, yaml).expect("write two-market yaml");
}

pub fn cycle_summary(response: &Value) -> &Value {
    response
        .get("cycle_summary")
        .expect("cycle_summary in daemon-once response")
}

pub fn audit_events_by_type(
    events: &[greenfloor_engine::storage::AuditEventRow],
) -> HashMap<String, Vec<&greenfloor_engine::storage::AuditEventRow>> {
    let mut out: HashMap<String, Vec<&greenfloor_engine::storage::AuditEventRow>> = HashMap::new();
    for event in events {
        out.entry(event.event_type.clone()).or_default().push(event);
    }
    out
}
