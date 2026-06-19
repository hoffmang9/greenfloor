#[path = "fixtures/daemon.rs"]
mod daemon_fixtures;
#[path = "fixtures/json_util.rs"]
mod json_util;

use daemon_fixtures::{
    audit_events_by_type, cycle_summary, daemon_request, run_daemon_once, write_daemon_program,
    write_markets_one, write_markets_two, DaemonRequestParams,
};
use greenfloor_engine::storage::SqliteStore;
use mockito::Matcher;
use serde_json::json;

const DAEMON_ENV: &[(&str, &str)] = &[
    ("GREENFLOOR_DAEMON_TEST_CONTROLS", "1"),
    ("GREENFLOOR_XCH_PRICE_USD", "30"),
];

fn setup_paths() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let state_dir = home.join("state");
    std::fs::create_dir_all(&state_dir).expect("create state dir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    let db_path = dir.path().join("state.sqlite");
    (dir, home, program, markets, db_path, state_dir)
}

#[tokio::test]
async fn daemon_multi_cycle_price_shift_cancel_and_reconcile() {
    let (_dir, home, program, markets, db_path, _state_dir) = setup_paths();
    let mut server = mockito::Server::new_async().await;
    let take_tx_id = "b".repeat(64);
    let offer_body = json!({
        "id": "offer-1",
        "status": 0,
        "tx_id": take_tx_id,
        "offered": [{"asset": "asset1", "amount": 1}],
        "requested": [{"asset": "xch", "amount": 1000}],
    });
    let _list = server
        .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
        .with_status(200)
        .with_body(json!({"success": true, "offers": [offer_body.clone()]}).to_string())
        .create_async()
        .await;
    let _get = server
        .mock("GET", "/v1/offers/offer-1")
        .with_status(200)
        .with_body(json!({"success": true, "offer": offer_body}).to_string())
        .create_async()
        .await;
    let _cancel = server
        .mock("POST", "/v1/offers/offer-1/cancel")
        .with_status(200)
        .with_body(r#"{"success":true,"id":"offer-1","status":3}"#)
        .create_async()
        .await;

    write_daemon_program(&program, &home, &server.url());
    write_markets_one(&markets, true);

    let store = SqliteStore::open(&db_path).expect("open db");
    store
        .upsert_offer_state("offer-1", "m1", "open", Some(0))
        .expect("seed offer");
    drop(store);

    let controls = json!({"skip_strategy_execution": true});
    let request = daemon_request(DaemonRequestParams {
        program: &program,
        markets: &markets,
        home: &home,
        db_path: &db_path,
        coinset_base: "http://coinset.local",
        poll_coinset_mempool: false,
        test_controls: controls.clone(),
    });
    let first = run_daemon_once(&request, DAEMON_ENV);
    assert_eq!(first.exit_code, 0, "stderr should be empty on success");

    let store = SqliteStore::open(&db_path).expect("open db");
    assert_eq!(
        store
            .observe_mempool_tx_ids(std::slice::from_ref(&take_tx_id))
            .expect("observe"),
        1
    );
    assert_eq!(store.confirm_tx_ids(&[take_tx_id]).expect("confirm"), 1);
    drop(store);

    let second_env = [
        ("GREENFLOOR_DAEMON_TEST_CONTROLS", "1"),
        ("GREENFLOOR_XCH_PRICE_USD", "40"),
    ];
    let second = run_daemon_once(&request, &second_env);
    assert_eq!(second.exit_code, 0);

    let store = SqliteStore::open(&db_path).expect("open db");
    let states = store.list_offer_states(Some("m1"), 10).expect("states");
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].offer_id, "offer-1");
    assert_eq!(states[0].state, "cancelled");
    assert_eq!(states[0].last_seen_status, Some(3));

    let events = store
        .list_recent_audit_events(
            Some(&[
                "offer_cancel_policy",
                "offer_lifecycle_transition",
                "daemon_cycle_summary",
            ]),
            None,
            30,
        )
        .expect("audit");
    drop(store);

    let by_type = audit_events_by_type(&events);
    assert!(by_type
        .get("offer_cancel_policy")
        .unwrap_or(&vec![])
        .iter()
        .any(|event| event.payload.get("triggered") == Some(&json!(true))));
    assert!(by_type
        .get("offer_lifecycle_transition")
        .unwrap_or(&vec![])
        .iter()
        .any(|event| event.payload.get("new_state") == Some(&json!("tx_block_confirmed"))));
    assert!(by_type.get("daemon_cycle_summary").map_or(0, Vec::len) >= 2);
}

#[test]
fn daemon_once_processes_multiple_markets() {
    let (_dir, home, program, markets, db_path, _state_dir) = setup_paths();
    write_daemon_program(&program, &home, "https://api.dexie.space");
    write_markets_two(&markets);

    let request = daemon_request(DaemonRequestParams {
        program: &program,
        markets: &markets,
        home: &home,
        db_path: &db_path,
        coinset_base: "https://coinset.org",
        poll_coinset_mempool: false,
        test_controls: json!({"skip_strategy_execution": true}),
    });
    let result = run_daemon_once(&request, DAEMON_ENV);
    assert_eq!(result.exit_code, 0);
    let summary = cycle_summary(result.response.as_ref().expect("response"));
    assert_eq!(summary.get("markets_processed"), Some(&json!(2)));
    assert_eq!(summary.get("markets_attempted"), Some(&json!(2)));
}

#[test]
fn daemon_once_isolates_forced_market_error() {
    let (_dir, home, program, markets, db_path, _state_dir) = setup_paths();
    write_daemon_program(&program, &home, "https://api.dexie.space");
    write_markets_two(&markets);

    let request = daemon_request(DaemonRequestParams {
        program: &program,
        markets: &markets,
        home: &home,
        db_path: &db_path,
        coinset_base: "https://coinset.org",
        poll_coinset_mempool: false,
        test_controls: json!({
            "skip_strategy_execution": true,
            "force_market_error_for": "m1",
        }),
    });
    let result = run_daemon_once(&request, DAEMON_ENV);
    assert_eq!(result.exit_code, 0);
    let summary = cycle_summary(result.response.as_ref().expect("response"));
    assert_eq!(summary.get("markets_attempted"), Some(&json!(2)));
    assert_eq!(summary.get("markets_processed"), Some(&json!(1)));
    assert!(
        summary
            .get("error_count")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            >= 1
    );
}

#[test]
fn daemon_once_sequential_slot_rotation_picks_up_new_market_next_cycle() {
    let (_dir, home, program, markets, db_path, _state_dir) = setup_paths();
    write_daemon_program(&program, &home, "https://api.dexie.space");
    write_markets_one(&markets, false);

    let controls = json!({"skip_strategy_execution": true});
    let mut request = daemon_request(DaemonRequestParams {
        program: &program,
        markets: &markets,
        home: &home,
        db_path: &db_path,
        coinset_base: "https://coinset.org",
        poll_coinset_mempool: false,
        test_controls: controls.clone(),
    });
    assert_eq!(run_daemon_once(&request, DAEMON_ENV).exit_code, 0);

    write_markets_two(&markets);
    assert_eq!(run_daemon_once(&request, DAEMON_ENV).exit_code, 0);

    let store = SqliteStore::open(&db_path).expect("open db");
    let events = store
        .list_recent_audit_events(Some(&["daemon_cycle_summary"]), None, 2)
        .expect("audit");
    drop(store);

    let mut attempted = events
        .iter()
        .map(|event| {
            event
                .payload
                .get("markets_attempted")
                .and_then(serde_json::Value::as_i64)
                .expect("markets_attempted")
        })
        .collect::<Vec<_>>();
    let mut processed = events
        .iter()
        .map(|event| {
            event
                .payload
                .get("markets_processed")
                .and_then(serde_json::Value::as_i64)
                .expect("markets_processed")
        })
        .collect::<Vec<_>>();
    attempted.sort_unstable();
    processed.sort_unstable();
    assert_eq!(attempted, vec![1, 2]);
    assert_eq!(processed, vec![1, 2]);

    let _ = &mut request;
}

#[test]
fn daemon_once_all_markets_fail_exits_non_zero() {
    let (_dir, home, program, markets, db_path, _state_dir) = setup_paths();
    write_daemon_program(&program, &home, "https://api.dexie.space");
    write_markets_one(&markets, false);

    let request = daemon_request(DaemonRequestParams {
        program: &program,
        markets: &markets,
        home: &home,
        db_path: &db_path,
        coinset_base: "https://coinset.org",
        poll_coinset_mempool: false,
        test_controls: json!({
            "skip_strategy_execution": true,
            "force_market_error_for": "m1",
        }),
    });
    assert_eq!(run_daemon_once(&request, DAEMON_ENV).exit_code, 1);
}

fn max_daemon_cycle_seconds() -> f64 {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => 2.0,
        _ => 1.5,
    }
}

#[test]
fn daemon_once_completes_within_cycle_time_budget() {
    let (_dir, home, program, markets, db_path, _state_dir) = setup_paths();
    let mut server = mockito::Server::new();
    let _offers = server
        .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
        .with_status(200)
        .with_body(r#"{"success":true,"offers":[]}"#)
        .create();

    let dexie_base = server.url();
    write_daemon_program(&program, &home, &dexie_base);
    write_markets_one(&markets, false);

    let request = daemon_request(DaemonRequestParams {
        program: &program,
        markets: &markets,
        home: &home,
        db_path: &db_path,
        coinset_base: "https://coinset.org",
        poll_coinset_mempool: false,
        test_controls: json!({"skip_strategy_execution": true}),
    });

    let started = std::time::Instant::now();
    let result = run_daemon_once(&request, DAEMON_ENV);
    let elapsed = started.elapsed();

    assert_eq!(result.exit_code, 0);
    let budget = max_daemon_cycle_seconds();
    assert!(
        elapsed.as_secs_f64() < budget,
        "daemon-once took {:.3}s, budget {budget:.1}s",
        elapsed.as_secs_f64()
    );
}
