use std::time::Instant;

use serde_json::{json, Value};

use crate::adapters::{DexieClient, SplashClient};
use crate::config::bake_expiry_into_conditions_for_quote;
use crate::error::SignerResult;
use crate::offer::codec::verify_offer_for_post;
use crate::offer::publish::expected_publish_asset_fields;

use super::context::ResolvedBuildAndPostContext;
use super::create::create_offer;
use super::publish::{
    finalize_publish_payload, offer_post_persist_record, publish_offer, CoinsetPublishEndpoint,
};
use super::types::{timing_payload, PostAttemptSuccess, PostFailure, PostIterationOutcome};
use super::BuildAndPostOfferRequest;
use crate::metrics::metric_millis_to_u64;
use crate::offer::action::BuildOfferForActionResult;
use crate::offer::operator::signer_denomination::{
    run_signer_denomination_phase, BootstrapPhaseResult,
};
use crate::offer::operator::{UniqueMakerPinRequest, UniqueMakerPinSession};
use crate::offer::types::effective_maker_reuse;

async fn run_bootstrap_phase(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
) -> SignerResult<(Value, Option<BootstrapPhaseResult>)> {
    let bootstrap_result = if request.run.dry_run {
        BootstrapPhaseResult::skipped("dry_run")
    } else if effective_maker_reuse(request.maker_reuse.as_ref()).is_some() {
        // PresplitExisting funds from the reused maker, not receive inventory.
        BootstrapPhaseResult::skipped("maker_reuse")
    } else {
        run_signer_denomination_phase(ctx).await?
    };
    let bootstrap_action = bootstrap_result.to_operator_json();
    Ok((bootstrap_action, Some(bootstrap_result)))
}

fn post_phase_failure(
    error: impl Into<String>,
    started: Instant,
    create_phase_ms: Option<u64>,
) -> PostIterationOutcome {
    PostIterationOutcome::Failure(PostFailure {
        error: error.into(),
        started,
        create_phase_ms,
        execution_mode: None,
        bootstrap: None,
    })
}

fn publish_succeeded(outcome: &PostIterationOutcome) -> bool {
    matches!(
        outcome,
        PostIterationOutcome::Success(success) if success.success
    )
}

async fn create_offer_for_post(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    started: Instant,
    offer_coin_ids: &[String],
) -> SignerResult<Result<(BuildOfferForActionResult, u64), PostIterationOutcome>> {
    let create_started = Instant::now();
    let created = match create_offer(
        ctx,
        request.size_base_units,
        request.maker_reuse.clone(),
        offer_coin_ids,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok(Err(post_phase_failure(
                err.to_string(),
                started,
                Some(metric_millis_to_u64(create_started.elapsed().as_millis())),
            )));
        }
    };
    let create_phase_ms = metric_millis_to_u64(create_started.elapsed().as_millis());

    if created.offer_text.trim().is_empty() {
        return Ok(Err(PostIterationOutcome::Failure(PostFailure {
            error: "signer_offer_text_unavailable".to_string(),
            started,
            create_phase_ms: Some(create_phase_ms),
            execution_mode: Some(created.execution_mode),
            bootstrap: None,
        })));
    }

    if request.run.dry_run {
        let offer_text = created.offer_text.trim();
        return Ok(Err(PostIterationOutcome::Preview(json!({
            "offer_prefix": &offer_text[..offer_text.len().min(24)],
            "offer_length": offer_text.len().to_string(),
        }))));
    }

    if let Some(verify_error) = verify_offer_for_post(
        &created.offer_text,
        bake_expiry_into_conditions_for_quote(&ctx.gated.market_row.quote_asset_type),
    ) {
        return Ok(Err(PostIterationOutcome::Failure(PostFailure {
            error: verify_error,
            started,
            create_phase_ms: Some(create_phase_ms),
            execution_mode: None,
            bootstrap: None,
        })));
    }

    Ok(Ok((created, create_phase_ms)))
}

#[allow(clippy::too_many_arguments)]
async fn publish_created_offer(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    created: BuildOfferForActionResult,
    create_phase_ms: u64,
    started: Instant,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
    coinset: Option<CoinsetPublishEndpoint<'_>>,
) -> SignerResult<PostIterationOutcome> {
    let side = created.side.as_str();
    let asset_fields = expected_publish_asset_fields(
        side,
        &ctx.gated.market_row.base_symbol,
        &ctx.offer_assets.quote_asset_for_offer,
        &ctx.offer_assets.base_asset_id,
        &ctx.offer_assets.quote_asset_id,
    );
    let publish_started = Instant::now();
    let publish = publish_offer(
        &ctx.publish_venue,
        dexie,
        splash,
        coinset,
        created.offer_text.trim(),
        request.venue.drop_only,
        request.venue.claim_rewards,
        &asset_fields,
    )
    .await?;
    let publish_ms = metric_millis_to_u64(publish_started.elapsed().as_millis());

    let persist_record = offer_post_persist_record(
        &publish,
        side,
        Some(created.execution_mode),
        ctx,
        request.size_base_units,
        Some(created.expires_at_unix),
        created.create_result.as_ref(),
    );
    let publish_success = publish.success;
    let result_payload = finalize_publish_payload(
        publish,
        created.execution_mode,
        timing_payload(
            started,
            Some(create_phase_ms),
            Some(create_phase_ms),
            Some(publish_ms),
        ),
        if ctx.publish_venue == "dexie" {
            Some(ctx.dexie_base_url.as_str())
        } else {
            None
        },
    );

    Ok(PostIterationOutcome::Success(Box::new(
        PostAttemptSuccess {
            publish_venue: ctx.publish_venue.clone(),
            result: result_payload,
            success: publish_success,
            persist_record,
        },
    )))
}

pub(super) async fn run_post_iteration(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
    session: &mut UniqueMakerPinSession,
) -> SignerResult<(Value, PostIterationOutcome)> {
    let started = Instant::now();

    let (bootstrap_action, bootstrap_result) = run_bootstrap_phase(request, ctx).await?;
    if let Some(bootstrap_result) = bootstrap_result {
        if let Some(error) = bootstrap_result.offer_creation_block_error() {
            return Ok((
                bootstrap_action,
                PostIterationOutcome::Failure(PostFailure {
                    error,
                    started,
                    create_phase_ms: None,
                    execution_mode: None,
                    bootstrap: Some(bootstrap_result.to_operator_json()),
                }),
            ));
        }
    }

    let side = ctx.action_side();
    if let Err(err) = session
        .pin_after_bootstrap(UniqueMakerPinRequest {
            market: &ctx.gated.market_row,
            assets: &ctx.offer_assets,
            size_base_units: request.size_base_units,
            side: &side,
            operator_network: &ctx.gated.operator_network,
            signer: &ctx.gated.signer,
            #[cfg(test)]
            test_pin_result: ctx.test_overrides.unique_pin_result.as_ref(),
        })
        .await
    {
        return Ok((
            bootstrap_action,
            post_phase_failure(err.to_string(), started, None),
        ));
    }
    let (created, create_phase_ms) =
        match create_offer_for_post(request, ctx, started, session.pending_offer_coin_ids()).await?
        {
            Ok(values) => values,
            Err(outcome) => {
                session.commit_on_publish(false);
                return Ok((bootstrap_action, outcome));
            }
        };

    let coinset = if ctx.publish_venue == "coinset" {
        let base = ctx.gated.signer.coinset_base_url.trim();
        Some(CoinsetPublishEndpoint {
            network: ctx.gated.operator_network.as_str(),
            base_url: if base.is_empty() { None } else { Some(base) },
        })
    } else {
        None
    };
    let outcome = publish_created_offer(
        request,
        ctx,
        created,
        create_phase_ms,
        started,
        dexie,
        splash,
        coinset,
    )
    .await?;
    session.commit_on_publish(publish_succeeded(&outcome));

    Ok((bootstrap_action, outcome))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use serde_json::{json, Value};

    use super::create_offer_for_post;
    use super::PostIterationOutcome;
    use crate::config::bake_expiry_into_conditions_for_quote;
    use crate::offer::codec::verify_offer_for_post;
    use crate::offer::operator::build_and_post::context::sample_resolved_build_and_post_context;
    use crate::offer::operator::UniqueMakerPinSession;
    use crate::test_support::build_and_post::unused_post_iteration_request;

    #[tokio::test]
    async fn create_offer_for_post_rejects_unverifiable_offer_text() {
        let mut ctx = sample_resolved_build_and_post_context();
        ctx.test_overrides.offer_text = Some("not-an-offer".to_string());
        let offer_text = "not-an-offer";
        let request = unused_post_iteration_request(false, Some(offer_text));
        let expected_verify_error = verify_offer_for_post(
            offer_text,
            bake_expiry_into_conditions_for_quote(&ctx.gated.market_row.quote_asset_type),
        )
        .expect("verify error");

        let outcome = create_offer_for_post(&request, &ctx, Instant::now(), &[])
            .await
            .expect("iteration result")
            .expect_err("verify failure");

        match outcome {
            PostIterationOutcome::Failure(failure) => {
                assert_eq!(failure.error, expected_verify_error);
            }
            _ => panic!("expected verify failure"),
        }
    }

    #[tokio::test]
    async fn create_offer_for_post_rejects_empty_offer_text() {
        let mut ctx = sample_resolved_build_and_post_context();
        ctx.test_overrides.offer_text = Some(String::new());
        let request = unused_post_iteration_request(false, None);

        let outcome = create_offer_for_post(&request, &ctx, Instant::now(), &[])
            .await
            .expect("iteration result")
            .expect_err("empty offer");

        match outcome {
            PostIterationOutcome::Failure(failure) => {
                assert_eq!(failure.error, "signer_offer_text_unavailable");
            }
            _ => panic!("expected empty-offer failure"),
        }
    }

    #[tokio::test]
    async fn run_post_iteration_blocks_when_bootstrap_not_ready() {
        let ctx = sample_resolved_build_and_post_context();
        let request = unused_post_iteration_request(false, Some("offer1dryrunpreviewstub"));
        let mut session = UniqueMakerPinSession::inactive();

        let (_bootstrap_action, outcome) =
            super::run_post_iteration(&request, &ctx, None, None, &mut session)
                .await
                .expect("iteration");

        match outcome {
            PostIterationOutcome::Failure(failure) => {
                assert!(
                    failure.error.contains("missing_sell_ladder"),
                    "unexpected error: {}",
                    failure.error
                );
                assert!(failure.create_phase_ms.is_none());
            }
            _ => panic!("expected bootstrap block failure"),
        }
    }

    #[tokio::test]
    async fn run_post_iteration_skips_bootstrap_for_maker_reuse() {
        let mut ctx = sample_resolved_build_and_post_context();
        let offer_text = "not-an-offer";
        ctx.test_overrides.offer_text = Some(offer_text.to_string());
        let mut request = unused_post_iteration_request(false, Some(offer_text));
        request.maker_reuse = Some(crate::offer::types::PresplitMakerReuse {
            coin_id: "aa".repeat(32),
            offer_nonce: "bb".repeat(32),
        });
        let expected_verify_error = verify_offer_for_post(
            offer_text,
            bake_expiry_into_conditions_for_quote(&ctx.gated.market_row.quote_asset_type),
        )
        .expect("verify error");
        let mut session = UniqueMakerPinSession::inactive();

        let (bootstrap_action, outcome) =
            super::run_post_iteration(&request, &ctx, None, None, &mut session)
                .await
                .expect("iteration");

        assert_eq!(
            bootstrap_action.get("reason").and_then(Value::as_str),
            Some("maker_reuse")
        );
        match outcome {
            PostIterationOutcome::Failure(failure) => {
                assert_eq!(failure.error, expected_verify_error);
            }
            _ => panic!("expected post-bootstrap verify failure"),
        }
    }

    #[tokio::test]
    async fn run_post_iteration_bootstraps_when_maker_reuse_unusable() {
        let ctx = sample_resolved_build_and_post_context();
        let mut request = unused_post_iteration_request(false, Some("offer1dryrunpreviewstub"));
        request.maker_reuse = Some(crate::offer::types::PresplitMakerReuse {
            coin_id: String::new(),
            offer_nonce: "bb".repeat(32),
        });
        let mut session = UniqueMakerPinSession::inactive();

        let (_bootstrap_action, outcome) =
            super::run_post_iteration(&request, &ctx, None, None, &mut session)
                .await
                .expect("iteration");

        match outcome {
            PostIterationOutcome::Failure(failure) => {
                assert!(
                    failure.error.contains("missing_sell_ladder"),
                    "empty coin_id must not skip bootstrap: {}",
                    failure.error
                );
            }
            _ => panic!("expected bootstrap block for unusable maker_reuse"),
        }
    }

    #[tokio::test]
    async fn pin_after_bootstrap_errors_leave_pending_clear() {
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let store = crate::storage::SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut session =
            UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        let ctx = sample_resolved_build_and_post_context();
        let stub = Err("no_free_exact_maker".to_string());
        let side = ctx.action_side();
        let err = session
            .pin_after_bootstrap(crate::offer::operator::UniqueMakerPinRequest {
                market: &ctx.gated.market_row,
                assets: &ctx.offer_assets,
                size_base_units: 10,
                side: &side,
                operator_network: &ctx.gated.operator_network,
                signer: &ctx.gated.signer,
                test_pin_result: Some(&stub),
            })
            .await
            .expect_err("stub err");
        assert_eq!(err.to_string(), "no_free_exact_maker");
        assert!(session.pending_offer_coin_ids().is_empty());
        assert!(!session.has_exclude("aa".repeat(32).as_str()));
    }

    #[tokio::test]
    async fn pin_after_bootstrap_does_not_commit_session_on_pin() {
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let store = crate::storage::SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut session =
            UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        let ctx = sample_resolved_build_and_post_context();
        let coin = "aa".repeat(32);
        let stub = Ok(coin.clone());
        let side = ctx.action_side();
        session
            .pin_after_bootstrap(crate::offer::operator::UniqueMakerPinRequest {
                market: &ctx.gated.market_row,
                assets: &ctx.offer_assets,
                size_base_units: 10,
                side: &side,
                operator_network: &ctx.gated.operator_network,
                signer: &ctx.gated.signer,
                test_pin_result: Some(&stub),
            })
            .await
            .expect("pin");
        assert_eq!(
            session.pending_offer_coin_ids(),
            std::slice::from_ref(&coin)
        );
        assert!(
            !session.has_exclude(&coin),
            "pin alone must not reserve; commit only after publish success"
        );
    }

    #[tokio::test]
    async fn pin_after_bootstrap_live_pins_via_coinset() {
        use crate::config::MarketPricing;
        use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
        use crate::offer::ResolvedMarketOfferAssets;
        use crate::storage::{OfferCancelWrite, OfferListingWrite};
        use crate::test_support::signer_config::test_signer_config;
        use tempfile::tempdir;

        const RECEIVE: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
        let cat = "ab".repeat(32);
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[]}"#)
            .create_async()
            .await;

        let dir = tempdir().expect("tempdir");
        let store = crate::storage::SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let prebound = "aa".repeat(32);
        let fields = OfferCancelFields::from_presplit_build(
            prebound.clone(),
            "bb".repeat(32),
            "cc".repeat(32),
        );
        store
            .upsert_offer_state_with_metadata_at(
                "o1",
                "m1",
                "open",
                None,
                "2026-01-01T00:00:00Z",
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::Direct),
                    listing: OfferListingWrite {
                        publish_venue: Some("dexie"),
                        listing_expires_at: None,
                        size_base_units: Some(10),
                        offer_nonce: None,
                        offer_side: Some("sell"),
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("seed");
        let mut session =
            UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        assert!(session.has_exclude(&prebound));

        let mut ctx = sample_resolved_build_and_post_context();
        ctx.gated.signer = test_signer_config(&server.url());
        ctx.gated.market_row.receive_address = RECEIVE.to_string();
        ctx.gated.market_row.base_asset = cat.clone();
        ctx.gated.market_row.pricing = MarketPricing {
            fixed_quote_per_base: Some(1.0),
            base_unit_mojo_multiplier: Some(1_000),
            quote_unit_mojo_multiplier: Some(1_000_000_000_000),
            ..MarketPricing::default()
        };
        ctx.offer_assets = ResolvedMarketOfferAssets {
            base_asset_id: cat,
            quote_asset_id: "xch".to_string(),
            quote_asset_for_offer: "xch".to_string(),
        };
        let side = ctx.action_side();
        let err = session
            .pin_after_bootstrap(crate::offer::operator::UniqueMakerPinRequest {
                market: &ctx.gated.market_row,
                assets: &ctx.offer_assets,
                size_base_units: 10,
                side: &side,
                operator_network: &ctx.gated.operator_network,
                signer: &ctx.gated.signer,
                test_pin_result: None,
            })
            .await
            .expect_err("empty Coinset");
        assert_eq!(err.to_string(), "no unspent cat coins");
        assert!(
            session.has_exclude(&prebound),
            "soft pin failure must leave session excludes unchanged"
        );
    }

    #[tokio::test]
    async fn pin_after_bootstrap_errors_on_invalid_offered_leg() {
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let store = crate::storage::SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut session =
            UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        let mut ctx = sample_resolved_build_and_post_context();
        ctx.gated.market_row.pricing.fixed_quote_per_base = Some(0.0);
        let side = ctx.action_side();
        let err = session
            .pin_after_bootstrap(crate::offer::operator::UniqueMakerPinRequest {
                market: &ctx.gated.market_row,
                assets: &ctx.offer_assets,
                size_base_units: 10,
                side: &side,
                operator_network: &ctx.gated.operator_network,
                signer: &ctx.gated.signer,
                test_pin_result: None,
            })
            .await
            .expect_err("bad pricing");
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn commit_on_publish_only_after_successful_publish() {
        use tempfile::tempdir;

        let dir = tempdir().expect("tempdir");
        let store = crate::storage::SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut session =
            UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        let ctx = sample_resolved_build_and_post_context();
        let coin = "bb".repeat(32);
        let stub = Ok(coin.clone());
        let side = ctx.action_side();
        session
            .pin_after_bootstrap(crate::offer::operator::UniqueMakerPinRequest {
                market: &ctx.gated.market_row,
                assets: &ctx.offer_assets,
                size_base_units: 10,
                side: &side,
                operator_network: &ctx.gated.operator_network,
                signer: &ctx.gated.signer,
                test_pin_result: Some(&stub),
            })
            .await
            .expect("pin");
        let failure = PostIterationOutcome::Failure(super::PostFailure {
            error: "create_failed".to_string(),
            started: Instant::now(),
            create_phase_ms: None,
            execution_mode: None,
            bootstrap: None,
        });
        session.commit_on_publish(super::publish_succeeded(&failure));
        assert!(!session.has_exclude(&coin));
        assert!(session.pending_offer_coin_ids().is_empty());

        session
            .pin_after_bootstrap(crate::offer::operator::UniqueMakerPinRequest {
                market: &ctx.gated.market_row,
                assets: &ctx.offer_assets,
                size_base_units: 10,
                side: &side,
                operator_network: &ctx.gated.operator_network,
                signer: &ctx.gated.signer,
                test_pin_result: Some(&stub),
            })
            .await
            .expect("pin");
        let publish_failed = PostIterationOutcome::Success(Box::new(super::PostAttemptSuccess {
            publish_venue: "dexie".to_string(),
            result: json!({}),
            success: false,
            persist_record: None,
        }));
        session.commit_on_publish(super::publish_succeeded(&publish_failed));
        assert!(!session.has_exclude(&coin));

        session
            .pin_after_bootstrap(crate::offer::operator::UniqueMakerPinRequest {
                market: &ctx.gated.market_row,
                assets: &ctx.offer_assets,
                size_base_units: 10,
                side: &side,
                operator_network: &ctx.gated.operator_network,
                signer: &ctx.gated.signer,
                test_pin_result: Some(&stub),
            })
            .await
            .expect("pin");
        let published = PostIterationOutcome::Success(Box::new(super::PostAttemptSuccess {
            publish_venue: "dexie".to_string(),
            result: json!({}),
            success: true,
            persist_record: None,
        }));
        session.commit_on_publish(super::publish_succeeded(&published));
        assert!(session.has_exclude(&coin));
        assert!(session.pending_offer_coin_ids().is_empty());
    }
}
