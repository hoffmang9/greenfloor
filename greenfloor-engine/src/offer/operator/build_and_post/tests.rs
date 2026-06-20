use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use super::context::{resolve_action_side, sample_resolved_build_and_post_context};
use super::post_batch::{
    apply_post_iteration_outcome, flush_post_batch, PostEmitTarget, PostFailureAudit,
    PostIterationBatch,
};
use super::publish::offer_post_persist_record;
use super::types::{build_and_post_exit_code, PostAttemptSuccess, PostFailure, PublishResult};
use crate::cli_util::{format_json, format_json_value};
use crate::operator_log::OFFER_POST_FAILURE;
use crate::storage::{state_db_path_for_home, SqliteStore};

#[test]
fn flush_post_batch_writes_failure_audit_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(&home);
    let store = SqliteStore::open(&db_path).expect("open");
    let ctx = sample_resolved_build_and_post_context();
    flush_post_batch(
        &store,
        &ctx,
        &[],
        &[PostFailureAudit {
            error: "dexie_http_error:500".to_string(),
            offer_ref: None,
        }],
    )
    .expect("persist failure");
    let events = store
        .list_recent_audit_events(Some(&[OFFER_POST_FAILURE]), Some("m1"), 1)
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].payload.get("error").and_then(Value::as_str),
        Some("dexie_http_error:500")
    );
}

#[test]
fn dry_run_failure_traces_without_persisting() {
    let capture = crate::operator_log::TraceCapture::install();
    let ctx = sample_resolved_build_and_post_context();
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist_records: Vec::new(),
        failure_audits: Vec::new(),
    };
    apply_post_iteration_outcome(
        PostEmitTarget::TraceOnly,
        &ctx,
        super::types::PostIterationOutcome::Failure(PostFailure {
            error: "dexie_http_error:500".to_string(),
            started: Instant::now(),
            create_phase_ms: None,
            execution_mode: None,
            bootstrap: None,
        }),
        &mut batch,
    );
    assert_eq!(batch.failure_audits.len(), 0);
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 1);
    assert_eq!(
        PostEmitTarget::from_run(true, true),
        PostEmitTarget::TraceOnly
    );
}

#[test]
fn persist_path_defers_failure_trace_until_flush() {
    let capture = crate::operator_log::TraceCapture::install();
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(&home);
    let store = SqliteStore::open(&db_path).expect("open");
    let ctx = sample_resolved_build_and_post_context();
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist_records: Vec::new(),
        failure_audits: Vec::new(),
    };
    apply_post_iteration_outcome(
        PostEmitTarget::TraceAndStore,
        &ctx,
        super::types::PostIterationOutcome::Failure(PostFailure {
            error: "dexie_http_error:500".to_string(),
            started: Instant::now(),
            create_phase_ms: None,
            execution_mode: None,
            bootstrap: None,
        }),
        &mut batch,
    );
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 0);
    assert_eq!(batch.failure_audits.len(), 1);
    flush_post_batch(&store, &ctx, &[], &batch.failure_audits).expect("persist");
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 1);
}

#[test]
fn cli_json_formatting_respects_compact_flag() {
    let payload = json!({"ok": true});
    assert!(format_json(&payload, false).unwrap().contains('\n'));
    assert_eq!(format_json_value(&payload, true).unwrap(), r#"{"ok":true}"#);
}

#[test]
fn offer_post_persist_record_requires_success_and_offer_id() {
    let ctx = sample_resolved_build_and_post_context();
    let failed = PublishResult {
        success: false,
        offer_id: Some("offer-1".to_string()),
        body: json!({"success": false}),
    };
    assert!(offer_post_persist_record(&failed, "sell", "direct", &ctx, 1).is_none());

    let success = PublishResult {
        success: true,
        offer_id: Some("offer-1".to_string()),
        body: json!({"success": true, "id": "offer-1"}),
    };
    let record = offer_post_persist_record(&success, "sell", "direct", &ctx, 10).expect("record");
    assert_eq!(record.offer_id, "offer-1");
    assert_eq!(record.market_id, "m1");
}

#[test]
fn post_attempt_success_tracks_publish_outcome_without_json_reparse() {
    let success = PostAttemptSuccess {
        publish_venue: "dexie".to_string(),
        result: json!({"success": false, "error": "dexie_http_error:500"}),
        success: false,
        persist_record: None,
    };
    assert!(!success.success);
    assert_eq!(
        success
            .to_venue_result()
            .get("result")
            .and_then(|value| value.get("error"))
            .and_then(Value::as_str),
        Some("dexie_http_error:500")
    );
}

#[test]
fn flush_post_batch_writes_offer_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(Path::new(&home));
    let store = SqliteStore::open(&db_path).expect("open");
    let ctx = sample_resolved_build_and_post_context();
    flush_post_batch(
        &store,
        &ctx,
        &[crate::storage::OfferPostPersistRecord {
            offer_id: "offer-abc".to_string(),
            market_id: "m1".to_string(),
            side: "sell".to_string(),
            size_base_units: 5,
            publish_venue: "dexie".to_string(),
            resolved_base_asset_id: "a1".to_string(),
            resolved_quote_asset_id: "xch".to_string(),
            created_extra: json!({"execution_mode": "direct"}),
        }],
        &[],
    )
    .expect("persist");

    assert_eq!(
        store
            .offer_state_for_id("offer-abc")
            .expect("state")
            .as_deref(),
        Some("open")
    );
}

#[test]
fn post_emit_target_skips_persist_for_dry_run() {
    assert_eq!(
        PostEmitTarget::from_run(true, true),
        PostEmitTarget::TraceOnly
    );
    assert_eq!(
        PostEmitTarget::from_run(true, false),
        PostEmitTarget::TraceAndStore
    );
}

#[test]
fn resolve_action_side_prefers_explicit_override() {
    let pricing = json!({"side": "sell"});
    assert_eq!(
        resolve_action_side(Some("buy"), &pricing),
        "buy".to_string()
    );
    assert_eq!(resolve_action_side(None, &pricing), "sell".to_string());
    assert_eq!(resolve_action_side(Some(""), &pricing), "sell".to_string());
}

#[test]
fn build_and_post_exit_code_reflects_publish_failures() {
    assert_eq!(build_and_post_exit_code(0), 0);
    assert_eq!(build_and_post_exit_code(1), 2);
    assert_eq!(build_and_post_exit_code(3), 2);
}

#[test]
fn post_failure_venue_result_marks_publish_failure() {
    let failure = PostFailure {
        error: "dexie_http_error:500".to_string(),
        started: Instant::now(),
        create_phase_ms: Some(12),
        execution_mode: Some("direct".to_string()),
        bootstrap: None,
    };
    let venue = failure.to_venue_result("dexie");
    assert_eq!(venue.get("venue").and_then(Value::as_str), Some("dexie"));
    assert_eq!(
        venue
            .get("result")
            .and_then(|value| value.get("success"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        venue
            .get("result")
            .and_then(|value| value.get("error"))
            .and_then(Value::as_str),
        Some("dexie_http_error:500")
    );
    assert_eq!(build_and_post_exit_code(1), 2);
}

fn write_dry_run_program(path: &Path, home_dir: &Path) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let mut text =
        std::fs::read_to_string(root.join("config/program.yaml")).expect("read program template");
    let home = home_dir.display().to_string();
    if text.contains("home_dir: \"~/.greenfloor\"") {
        text = text.replace(
            "home_dir: \"~/.greenfloor\"",
            &format!("home_dir: \"{home}\""),
        );
    } else {
        text = text.replacen("home_dir:", &format!("home_dir: \"{home}\""), 1);
    }
    if text.contains("kms_key_id: \"\"") {
        text = text.replace(
            "kms_key_id: \"\"",
            "kms_key_id: \"arn:aws:kms:us-west-2:123:key/demo\"",
        );
    }
    if text.contains("kms_region: \"\"") {
        text = text.replace("kms_region: \"\"", "kms_region: \"us-west-2\"");
    }
    if text.contains("kms_public_key_hex: \"\"") {
        text = text.replace(
            "kms_public_key_hex: \"\"",
            "kms_public_key_hex: \"02abc123\"",
        );
    }
    std::fs::write(path, text).expect("write signer program");
}

#[tokio::test]
async fn dry_run_returns_preview_payload_in_process() {
    std::env::set_var("GREENFLOOR_TEST_OFFER_TEXT", "offer1dryrunpreviewstub");
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_dry_run_program(&program, dir.path());
    let markets_yaml = r#"markets:
  - id: m1
    enabled: true
    base_asset: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    base_symbol: "TCAT"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#;
    std::fs::write(&markets, markets_yaml).expect("write markets");

    let response = super::build_and_post_offer(super::BuildAndPostOfferRequest {
        program_path: program,
        markets_path: markets,
        testnet_markets_path: None,
        network: "mainnet".to_string(),
        market_id: Some("m1".to_string()),
        pair: None,
        size_base_units: 1,
        repeat: 1,
        publish_venue: None,
        dexie_base_url: None,
        splash_base_url: None,
        venue: super::BuildAndPostVenueOptions {
            drop_only: true,
            claim_rewards: false,
        },
        run: super::BuildAndPostRunOptions {
            dry_run: true,
            persist_results: true,
        },
        action_side: None,
        test_overrides: crate::offer::operator::OfferOperatorTestOverrides::from_env(),
    })
    .await
    .expect("build and post dry run");

    assert_eq!(response.exit_code, 0);
    assert_eq!(response.payload.get("dry_run"), Some(&json!(true)));
    assert_eq!(response.payload.get("publish_attempts"), Some(&json!(0)));
    assert!(response
        .payload
        .get("built_offers_preview")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty()));
    assert_eq!(response.payload.get("results"), Some(&json!([])));
    std::env::remove_var("GREENFLOOR_TEST_OFFER_TEXT");
}
