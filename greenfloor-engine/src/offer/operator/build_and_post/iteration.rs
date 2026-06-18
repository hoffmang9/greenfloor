use std::time::Instant;

use serde_json::{json, Value};

use crate::adapters::{DexieClient, SplashClient};
use crate::error::SignerResult;
use crate::offer::codec::verify_offer_for_dexie;
use crate::offer::publish::expected_publish_asset_fields;

use crate::offer::operator::bootstrap::{
    bootstrap_blocks_offer, signer_bootstrap_phase, BootstrapPhaseResult,
};
use super::context::ResolvedBuildAndPostContext;
use super::create::create_offer;
use super::publish::{
    finalize_publish_payload, offer_post_persist_record, publish_offer,
};
use super::types::{
    PostAttemptSuccess, PostFailure, PostIterationOutcome, timing_payload,
};
use super::BuildAndPostOfferRequest;

pub(super) async fn run_post_iteration(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
) -> SignerResult<(Value, PostIterationOutcome)> {
    let started = Instant::now();

    let bootstrap_result = if request.dry_run {
        BootstrapPhaseResult::skipped("dry_run")
    } else {
        signer_bootstrap_phase(
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
    let bootstrap_action = bootstrap_result.to_manager_json();
    if let Some(error) = bootstrap_blocks_offer(&bootstrap_result) {
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Failure(PostFailure {
                error,
                started,
                create_phase_ms: None,
                execution_mode: None,
                bootstrap: Some(bootstrap_result.to_manager_json()),
            }),
        ));
    }

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
            return Ok((
                bootstrap_action,
                PostIterationOutcome::Failure(PostFailure {
                    error: err.to_string(),
                    started,
                    create_phase_ms: Some(create_started.elapsed().as_millis() as u64),
                    execution_mode: None,
                    bootstrap: None,
                }),
            ));
        }
    };
    let create_phase_ms = create_started.elapsed().as_millis() as u64;

    if created.offer_text.trim().is_empty() {
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Failure(PostFailure {
                error: "signer_offer_text_unavailable".to_string(),
                started,
                create_phase_ms: Some(create_phase_ms),
                execution_mode: Some(created.execution_mode.clone()),
                bootstrap: None,
            }),
        ));
    }

    if request.dry_run {
        let offer_text = created.offer_text.trim();
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Preview(json!({
                "offer_prefix": &offer_text[..offer_text.len().min(24)],
                "offer_length": offer_text.len().to_string(),
            })),
        ));
    }

    if let Some(verify_error) = verify_offer_for_dexie(&created.offer_text) {
        return Ok((
            bootstrap_action,
            PostIterationOutcome::Failure(PostFailure {
                error: verify_error,
                started,
                create_phase_ms: Some(create_phase_ms),
                execution_mode: None,
                bootstrap: None,
            }),
        ));
    }

    let side = created.side.as_str();
    let asset_fields = expected_publish_asset_fields(
        side,
        &ctx.market.base_symbol,
        &ctx.market.quote_asset,
        &ctx.resolved_base_asset_id,
        &ctx.resolved_quote_asset_id,
    );
    let publish_started = Instant::now();
    let publish = publish_offer(
        &ctx.publish_venue,
        dexie,
        splash,
        created.offer_text.trim(),
        request.drop_only,
        request.claim_rewards,
        &asset_fields.expected_offered_asset_id,
        &asset_fields.expected_offered_symbol,
        &asset_fields.expected_requested_asset_id,
        &asset_fields.expected_requested_symbol,
    )
    .await?;
    let publish_ms = publish_started.elapsed().as_millis() as u64;

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

    Ok((
        bootstrap_action,
        PostIterationOutcome::Success(PostAttemptSuccess {
            publish_venue: ctx.publish_venue.clone(),
            result: result_payload,
            success: publish_success,
            persist_record,
        }),
    ))
}
