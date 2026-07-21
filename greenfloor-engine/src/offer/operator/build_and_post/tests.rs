use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use super::context::{resolve_action_side, sample_resolved_build_and_post_context};
use super::post_batch::{
    apply_post_iteration_outcome, PostBatchEmitter, PostEmitTarget, PostFailureAudit,
    PostIterationBatch, PostPersistPayload,
};
use super::publish::offer_post_persist_record;
use super::types::{build_and_post_exit_code, PostAttemptSuccess, PostFailure, PublishResult};
use crate::offer::types::{CreateOfferResult, OfferCancelFields, OfferExecutionMode};
use crate::operator_log::OFFER_POST_FAILURE;
use crate::storage::{
    state_db_path_for_home, upsert_offer_post_record, CycleWriteStore, SqliteStore,
};
use crate::test_support::build_and_post::unused_post_iteration_request;
use crate::test_support::minimal_program::{
    write_minimal_program_with_signer, MinimalProgramParams,
};

#[test]
fn flush_post_batch_writes_failure_audit_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(&home);
    let store = SqliteStore::open(&db_path).expect("open");
    let ctx = sample_resolved_build_and_post_context();
    let emitter = PostBatchEmitter::new(&ctx);
    emitter
        .flush_audits(
            &store,
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
    let emitter = PostBatchEmitter::new(&ctx);
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist: PostPersistPayload {
            persist_records: Vec::new(),
            failure_audits: Vec::new(),
        },
    };
    apply_post_iteration_outcome(
        PostEmitTarget::TraceOnly,
        &emitter,
        super::types::PostIterationOutcome::Failure(PostFailure {
            error: "dexie_http_error:500".to_string(),
            started: Instant::now(),
            create_phase_ms: None,
            execution_mode: None,
            bootstrap: None,
        }),
        &mut batch,
        None,
    );
    assert_eq!(batch.persist.failure_audits.len(), 0);
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
    let emitter = PostBatchEmitter::new(&ctx);
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist: PostPersistPayload {
            persist_records: Vec::new(),
            failure_audits: Vec::new(),
        },
    };
    apply_post_iteration_outcome(
        PostEmitTarget::TraceAndStore,
        &emitter,
        super::types::PostIterationOutcome::Failure(PostFailure {
            error: "dexie_http_error:500".to_string(),
            started: Instant::now(),
            create_phase_ms: None,
            execution_mode: None,
            bootstrap: None,
        }),
        &mut batch,
        None,
    );
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 0);
    assert_eq!(batch.persist.failure_audits.len(), 1);
    emitter
        .flush_audits(&store, &[], &batch.persist.failure_audits)
        .expect("persist");
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 1);
}

#[test]
fn offer_post_persist_record_requires_success_and_offer_id() {
    let ctx = sample_resolved_build_and_post_context();
    let failed = PublishResult {
        success: false,
        offer_id: Some("offer-1".to_string()),
        body: json!({"success": false}),
    };
    assert!(offer_post_persist_record(&failed, "sell", "direct", &ctx, 1, None).is_none());

    let success = PublishResult {
        success: true,
        offer_id: Some("offer-1".to_string()),
        body: json!({"success": true, "id": "offer-1"}),
    };
    let record =
        offer_post_persist_record(&success, "sell", "direct", &ctx, 10, None).expect("record");
    assert_eq!(record.offer_id, "offer-1");
    assert_eq!(record.market_id, "m1");

    let selected = "aa".repeat(32);
    let presplit_coin = "bb".repeat(32);
    let input_coin = "cc".repeat(32);
    let p2 = "dd".repeat(32);
    let create = CreateOfferResult {
        offer: "offer1".to_string(),
        spend_bundle_hex: String::new(),
        selected_coin_ids: vec![selected.clone()],
        offer_nonce: String::new(),
        execution_mode: OfferExecutionMode::PresplitNew,
        split_spend_bundle_hex: None,
        presplit_coin_id: Some(presplit_coin.clone()),
        split_broadcast_status: None,
        cancel_fields: OfferCancelFields::from_presplit_build(
            input_coin.clone(),
            "aa".repeat(32),
            p2.clone(),
        ),
    };
    let presplit = offer_post_persist_record(&success, "sell", "direct", &ctx, 10, Some(&create))
        .expect("presplit record");
    assert_eq!(
        presplit.execution_mode,
        Some(OfferExecutionMode::PresplitNew)
    );
    assert_eq!(
        presplit.cancel_fields.input_coin_id.as_deref(),
        Some(input_coin.as_str())
    );
    assert_eq!(
        presplit.watched_coin_ids,
        vec![selected, presplit_coin, input_coin]
    );
    assert_eq!(presplit.watched_p2s, vec![p2]);

    let direct_coin = "ee".repeat(32);
    let direct_p2 = "ff".repeat(32);
    let direct_create = CreateOfferResult {
        offer: "offer1".to_string(),
        spend_bundle_hex: String::new(),
        selected_coin_ids: vec![direct_coin.clone()],
        offer_nonce: String::new(),
        execution_mode: OfferExecutionMode::Direct,
        split_spend_bundle_hex: None,
        presplit_coin_id: None,
        split_broadcast_status: None,
        cancel_fields: OfferCancelFields::from_direct_build(direct_coin.clone(), direct_p2.clone()),
    };
    let direct =
        offer_post_persist_record(&success, "sell", "direct", &ctx, 10, Some(&direct_create))
            .expect("direct record");
    assert_eq!(direct.execution_mode, Some(OfferExecutionMode::Direct));
    assert_eq!(
        direct.cancel_fields.input_coin_id.as_deref(),
        Some(direct_coin.as_str())
    );
    assert!(direct.cancel_fields.fixed_delegated_puzzle_hash.is_none());
    assert_eq!(direct.watched_coin_ids, vec![direct_coin]);
    assert_eq!(direct.watched_p2s, vec![direct_p2]);
    assert!(crate::offer::metadata_sufficient_for_coinset_cancel(Some(
        &crate::offer::types::StoredOfferCancelMetadata {
            fields: direct.cancel_fields.clone(),
            execution_mode: Some(OfferExecutionMode::Direct),
        }
    )));
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
fn flush_post_batch_writes_execution_audit_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(Path::new(&home));
    let store = SqliteStore::open(&db_path).expect("open");
    let ctx = sample_resolved_build_and_post_context();
    let emitter = PostBatchEmitter::new(&ctx);
    emitter
        .flush_audits(
            &store,
            &[crate::storage::OfferPostPersistRecord {
                offer_id: "offer-abc".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 5,
                publish_venue: "dexie".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({}),
                cancel_fields: OfferCancelFields::default(),
                execution_mode: Some(OfferExecutionMode::Direct),
                watched_coin_ids: Vec::new(),
                watched_p2s: Vec::new(),
            }],
            &[],
        )
        .expect("persist");

    assert!(store
        .offer_state_for_id("offer-abc")
        .expect("state")
        .is_none());
}

#[test]
fn success_persists_offer_state_immediately_before_flush() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let db_path = state_db_path_for_home(Path::new(&home));
    let store = CycleWriteStore::from_sqlite(SqliteStore::open(&db_path).expect("open"));
    let ctx = sample_resolved_build_and_post_context();
    let emitter = PostBatchEmitter::new(&ctx);
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist: PostPersistPayload {
            persist_records: Vec::new(),
            failure_audits: Vec::new(),
        },
    };
    let coin = "aa".repeat(32);
    let p2 = "bb".repeat(32);
    let persist_store = store.clone();
    let mut persist = move |record: &crate::storage::OfferPostPersistRecord| {
        persist_store.sync(|store| upsert_offer_post_record(store, record))
    };
    apply_post_iteration_outcome(
        PostEmitTarget::TraceAndStore,
        &emitter,
        super::types::PostIterationOutcome::Success(Box::new(PostAttemptSuccess {
            publish_venue: "coinset".to_string(),
            result: json!({"success": true, "id": "offer-immediate"}),
            success: true,
            persist_record: Some(crate::storage::OfferPostPersistRecord {
                offer_id: "offer-immediate".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 5,
                publish_venue: "coinset".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({}),
                cancel_fields: OfferCancelFields::default(),
                execution_mode: Some(OfferExecutionMode::Direct),
                watched_coin_ids: vec![coin.clone()],
                watched_p2s: vec![p2.clone()],
            }),
        })),
        &mut batch,
        Some(&mut persist),
    );
    assert_eq!(batch.publish_failures, 0);
    assert_eq!(
        store
            .sync(|store| store.offer_state_for_id("offer-immediate"))
            .expect("state")
            .as_deref(),
        Some("open"),
        "venue success must persist open before batch flush"
    );
    let hits = store
        .sync(|store| store.match_watch_keys(&[coin, p2]))
        .expect("watches");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].row.offer_id, "offer-immediate");
}

#[test]
fn immediate_persist_failure_defers_a_single_failure_emit() {
    let capture = crate::operator_log::TraceCapture::install();
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&state_db_path_for_home(dir.path())).expect("open");
    let ctx = sample_resolved_build_and_post_context();
    let emitter = PostBatchEmitter::new(&ctx);
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist: PostPersistPayload {
            persist_records: Vec::new(),
            failure_audits: Vec::new(),
        },
    };
    let mut persist = |_record: &crate::storage::OfferPostPersistRecord| {
        Err(crate::error::SignerError::Other(
            "sqlite is unavailable".to_string(),
        ))
    };

    apply_post_iteration_outcome(
        PostEmitTarget::TraceAndStore,
        &emitter,
        super::types::PostIterationOutcome::Success(Box::new(PostAttemptSuccess {
            publish_venue: "coinset".to_string(),
            result: json!({"success": true, "id": "offer-persist-failure"}),
            success: true,
            persist_record: Some(crate::storage::OfferPostPersistRecord {
                offer_id: "offer-persist-failure".to_string(),
                market_id: "m1".to_string(),
                side: "sell".to_string(),
                size_base_units: 5,
                publish_venue: "coinset".to_string(),
                resolved_base_asset_id: "a1".to_string(),
                resolved_quote_asset_id: "xch".to_string(),
                created_extra: json!({}),
                cancel_fields: OfferCancelFields::default(),
                execution_mode: Some(OfferExecutionMode::Direct),
                watched_coin_ids: Vec::new(),
                watched_p2s: Vec::new(),
            }),
        })),
        &mut batch,
        Some(&mut persist),
    );

    assert_eq!(batch.publish_failures, 1);
    assert_eq!(batch.persist.failure_audits.len(), 1);
    assert_eq!(batch.persist.persist_records.len(), 1);
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 0);
    emitter
        .flush_audits(
            &store,
            &batch.persist.persist_records,
            &batch.persist.failure_audits,
        )
        .expect("flush audits");
    assert_eq!(capture.count_substr(OFFER_POST_FAILURE), 1);
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

#[tokio::test]
async fn dry_run_returns_preview_payload_in_process() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_minimal_program_with_signer(
        &program,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    std::fs::write(
        &markets,
        include_str!("../../../../tests/fixtures/data/build_offer_markets.yaml"),
    )
    .expect("write markets fixture");

    let response = super::build_and_post_offer(super::BuildAndPostOfferRequest {
        program_path: program,
        markets_path: markets,
        testnet_markets_path: None,
        cats_path: None,
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
        test_overrides: crate::offer::operator::BuildOfferTestOverrides {
            offer_text: Some("offer1dryrunpreviewstub".to_string()),
        },
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
}

#[tokio::test]
async fn run_post_iteration_dry_run_skips_bootstrap_and_returns_preview() {
    let mut ctx = sample_resolved_build_and_post_context();
    ctx.test_overrides.offer_text = Some("offer1dryrunpreviewstub".to_string());
    let request = unused_post_iteration_request(true, Some("offer1dryrunpreviewstub"));

    let (bootstrap_action, outcome) =
        super::iteration::run_post_iteration(&request, &ctx, None, None)
            .await
            .expect("iteration");

    assert_eq!(
        bootstrap_action.get("reason").and_then(Value::as_str),
        Some("dry_run")
    );
    match outcome {
        super::types::PostIterationOutcome::Preview(preview) => {
            assert_eq!(
                preview.get("offer_length").and_then(Value::as_str),
                Some("23")
            );
        }
        _other => panic!("expected preview outcome"),
    }
}
