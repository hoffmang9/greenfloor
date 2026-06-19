use std::path::Path;

use crate::operator_log::{LogContext, OFFER_POST_COMPLETED, OFFER_POST_ITERATION};

use super::context::ResolvedBuildAndPostContext;
use super::publish::persist_post_failure_if_enabled;
use super::types::PostIterationOutcome;
use crate::operator_log::offer_log_ref;

pub(super) fn log_post_iteration(
    ctx: &ResolvedBuildAndPostContext,
    outcome: &str,
    publish_venue: &str,
    error: Option<&str>,
    offer_ref: Option<&str>,
) {
    let fields = (
        LogContext::OFFER_POST.service,
        OFFER_POST_ITERATION,
        LogContext::OFFER_POST.phase,
        ctx.market.market_id.as_str(),
        outcome,
        publish_venue,
        error.unwrap_or(""),
        offer_ref.unwrap_or(""),
    );
    if outcome == "failure" {
        tracing::warn!(
            service = fields.0,
            event = fields.1,
            phase = fields.2,
            market_id = fields.3,
            outcome = fields.4,
            publish_venue = fields.5,
            error = fields.6,
            offer_ref = fields.7,
            "offer post iteration"
        );
    } else {
        tracing::info!(
            service = fields.0,
            event = fields.1,
            phase = fields.2,
            market_id = fields.3,
            outcome = fields.4,
            publish_venue = fields.5,
            error = fields.6,
            offer_ref = fields.7,
            "offer post iteration"
        );
    }
}

pub(super) fn log_post_iteration_outcome(
    ctx: &ResolvedBuildAndPostContext,
    outcome: &PostIterationOutcome,
) {
    match outcome {
        PostIterationOutcome::Success(success) => {
            let offer_ref = success
                .persist_record
                .as_ref()
                .map(|record| offer_log_ref(&record.offer_id));
            let iteration_outcome = if success.success {
                "success"
            } else {
                "failure"
            };
            let error = if success.success {
                None
            } else {
                success
                    .result
                    .get("error")
                    .and_then(|value| value.as_str())
                    .or_else(|| {
                        success
                            .result
                            .get("message")
                            .and_then(|value| value.as_str())
                    })
            };
            log_post_iteration(
                ctx,
                iteration_outcome,
                &ctx.publish_venue,
                error,
                offer_ref.as_deref(),
            );
        }
        PostIterationOutcome::Failure(failure) => {
            log_post_iteration(
                ctx,
                "failure",
                &ctx.publish_venue,
                Some(&failure.error),
                None,
            );
        }
        PostIterationOutcome::Preview(preview) => {
            let offer_ref = preview
                .get("offer_prefix")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            log_post_iteration(ctx, "preview", &ctx.publish_venue, None, Some(offer_ref));
        }
    }
}

pub(super) fn record_post_failure_if_enabled(
    home_dir: &Path,
    persist_results: bool,
    dry_run: bool,
    market_id: &str,
    publish_venue: &str,
    error: &str,
    offer_id: Option<&str>,
) {
    let _ = persist_post_failure_if_enabled(
        home_dir,
        persist_results,
        dry_run,
        market_id,
        publish_venue,
        error,
        offer_id,
    );
}

pub(super) fn log_build_and_post_completed(
    ctx: &ResolvedBuildAndPostContext,
    publish_failures: u32,
    publish_attempts: usize,
    dry_run: bool,
    exit_code: i32,
) {
    let outcome = if publish_failures == 0 {
        "success"
    } else if publish_failures == u32::try_from(publish_attempts).unwrap_or(u32::MAX) {
        "failure"
    } else {
        "partial_failure"
    };
    let common = (
        LogContext::OFFER_POST.service,
        OFFER_POST_COMPLETED,
        LogContext::OFFER_POST.phase,
        ctx.market.market_id.as_str(),
        outcome,
        publish_attempts,
        publish_failures,
        dry_run,
    );
    if exit_code == 0 {
        tracing::info!(
            service = common.0,
            event = common.1,
            phase = common.2,
            market_id = common.3,
            outcome = common.4,
            publish_attempts = common.5,
            publish_failures = common.6,
            dry_run = common.7,
            "build-and-post-offer completed"
        );
    } else if outcome == "failure" {
        tracing::error!(
            service = common.0,
            event = common.1,
            phase = common.2,
            market_id = common.3,
            outcome = common.4,
            publish_attempts = common.5,
            publish_failures = common.6,
            dry_run = common.7,
            "build-and-post-offer completed"
        );
    } else {
        tracing::warn!(
            service = common.0,
            event = common.1,
            phase = common.2,
            market_id = common.3,
            outcome = common.4,
            publish_attempts = common.5,
            publish_failures = common.6,
            dry_run = common.7,
            "build-and-post-offer completed"
        );
    }
}
