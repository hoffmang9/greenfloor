use std::time::Instant;

use serde_json::{json, Value};

use crate::adapters::{DexieClient, SplashClient};
use crate::error::SignerResult;
use crate::offer::codec::verify_offer_for_dexie;
use crate::offer::publish::expected_publish_asset_fields;

use super::context::ResolvedBuildAndPostContext;
use super::create::create_offer;
use super::operator_log::{log_post_iteration, log_post_iteration_outcome};
use super::publish::{
    finalize_publish_payload, offer_post_persist_record, publish_offer, PublishOfferParams,
};
use super::types::{timing_payload, PostAttemptSuccess, PostFailure, PostIterationOutcome};
use super::BuildAndPostOfferRequest;
use crate::metrics::metric_millis_to_u64;
use crate::offer::action::BuildOfferForActionResult;
use crate::offer::operator::signer_denomination::{
    bootstrap_blocks_offer, run_signer_denomination_phase, BootstrapPhaseResult,
};

async fn run_bootstrap_phase(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
) -> SignerResult<(Value, Option<BootstrapPhaseResult>)> {
    let bootstrap_result = if request.run.dry_run {
        BootstrapPhaseResult::skipped("dry_run")
    } else {
        run_signer_denomination_phase(
            &ctx.program,
            &ctx.market,
            &ctx.signer_config,
            &ctx.resolved_base_asset_id,
            &ctx.resolved_quote_asset_id,
            ctx.quote_price,
            &ctx.action_side,
        )
        .await?
    };
    let bootstrap_action = bootstrap_result.to_operator_json();
    Ok((bootstrap_action, Some(bootstrap_result)))
}

async fn create_offer_for_post(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    started: Instant,
) -> SignerResult<Result<(BuildOfferForActionResult, u64), PostIterationOutcome>> {
    let create_started = Instant::now();
    let created = match create_offer(
        &ctx.signer_config,
        &ctx.market,
        request.size_base_units,
        ctx.quote_price,
        &ctx.action_side,
        &ctx.test_overrides,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok(Err(PostIterationOutcome::Failure(PostFailure {
                error: err.to_string(),
                started,
                create_phase_ms: Some(metric_millis_to_u64(create_started.elapsed().as_millis())),
                execution_mode: None,
                bootstrap: None,
            })));
        }
    };
    let create_phase_ms = metric_millis_to_u64(create_started.elapsed().as_millis());

    if created.offer_text.trim().is_empty() {
        return Ok(Err(PostIterationOutcome::Failure(PostFailure {
            error: "signer_offer_text_unavailable".to_string(),
            started,
            create_phase_ms: Some(create_phase_ms),
            execution_mode: Some(created.execution_mode.clone()),
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

    if let Some(verify_error) = verify_offer_for_dexie(&created.offer_text) {
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

async fn publish_created_offer(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    created: BuildOfferForActionResult,
    create_phase_ms: u64,
    started: Instant,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
) -> SignerResult<PostIterationOutcome> {
    let side = created.side.as_str();
    let asset_fields = expected_publish_asset_fields(
        side,
        &ctx.market.base_symbol,
        &ctx.market.quote_asset,
        &ctx.resolved_base_asset_id,
        &ctx.resolved_quote_asset_id,
    );
    let publish_started = Instant::now();
    let publish = publish_offer(PublishOfferParams {
        publish_venue: &ctx.publish_venue,
        dexie,
        splash,
        offer_text: created.offer_text.trim(),
        drop_only: request.venue.drop_only,
        claim_rewards: request.venue.claim_rewards,
        expected_offered_asset_id: &asset_fields.expected_offered_asset_id,
        expected_offered_symbol: &asset_fields.expected_offered_symbol,
        expected_requested_asset_id: &asset_fields.expected_requested_asset_id,
        expected_requested_symbol: &asset_fields.expected_requested_symbol,
    })
    .await?;
    let publish_ms = metric_millis_to_u64(publish_started.elapsed().as_millis());

    let persist_record = offer_post_persist_record(
        &publish,
        side,
        &created.execution_mode,
        ctx,
        request.size_base_units,
    );
    let publish_success = publish.success;
    let result_payload = finalize_publish_payload(
        publish,
        &created.execution_mode,
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

    Ok(PostIterationOutcome::Success(PostAttemptSuccess {
        publish_venue: ctx.publish_venue.clone(),
        result: result_payload,
        success: publish_success,
        persist_record,
    }))
}

pub(super) async fn run_post_iteration(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
) -> SignerResult<(Value, PostIterationOutcome)> {
    let started = Instant::now();

    let (bootstrap_action, bootstrap_result) = run_bootstrap_phase(request, ctx).await?;
    if let Some(bootstrap_result) = bootstrap_result {
        if let Some(error) = bootstrap_blocks_offer(&bootstrap_result) {
            log_post_iteration(ctx, "failure", &ctx.publish_venue, Some(&error), None);
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

    let (created, create_phase_ms) = match create_offer_for_post(request, ctx, started).await? {
        Ok(values) => values,
        Err(outcome) => {
            log_post_iteration_outcome(ctx, &outcome);
            return Ok((bootstrap_action, outcome));
        }
    };

    let outcome = publish_created_offer(
        request,
        ctx,
        created,
        create_phase_ms,
        started,
        dexie,
        splash,
    )
    .await?;

    log_post_iteration_outcome(ctx, &outcome);

    Ok((bootstrap_action, outcome))
}
