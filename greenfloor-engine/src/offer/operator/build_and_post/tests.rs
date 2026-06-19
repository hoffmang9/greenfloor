use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use super::context::{resolve_action_side, sample_resolved_build_and_post_context};
use super::publish::{
    log_post_iteration_outcome, offer_post_persist_record, persist_post_failure_audits,
    persist_post_records, PostAuditContext, PostFailureAudit,
};
use super::types::{build_and_post_exit_code, PostAttemptSuccess, PostFailure, PublishResult};
use crate::cli_util::{format_json, format_json_value};
use crate::operator_log::OFFER_POST_FAILURE;
use crate::storage::{state_db_path_for_home, SqliteStore};

#[test]
fn persist_post_failure_writes_audit_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(&home);
    let store = SqliteStore::open(&db_path).expect("open");
    let ctx = sample_resolved_build_and_post_context();
    persist_post_failure_audits(
        &store,
        &ctx,
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
fn persist_post_failure_skips_dry_run_audit() {
    let capture = crate::operator_log::TraceCapture::install();
    let ctx = sample_resolved_build_and_post_context();
    let failure = log_post_iteration_outcome(
        &ctx,
        &super::types::PostIterationOutcome::Failure(PostFailure {
            error: "dexie_http_error:500".to_string(),
            started: Instant::now(),
            create_phase_ms: None,
            execution_mode: None,
            bootstrap: None,
        }),
    );
    assert!(failure.is_some());
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 1);
    assert!(PostAuditContext {
        persist_results: true,
        dry_run: true,
    }
    .traces_only());
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
fn persist_post_records_writes_sqlite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(Path::new(&home));
    let store = SqliteStore::open(&db_path).expect("open");
    persist_post_records(
        &store,
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
fn persist_post_records_skips_dry_run() {
    assert!(PostAuditContext {
        persist_results: true,
        dry_run: true,
    }
    .traces_only());
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
