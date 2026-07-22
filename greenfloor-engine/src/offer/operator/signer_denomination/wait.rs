use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::coin_ops::execution::resolve_combine_input_cap;
use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::SignerConfig;
use crate::cycle::retry::{poll_exponential_advance_sleep, poll_exponential_sleep_now};
use crate::error::{SignerError, SignerResult};
use crate::offer::bootstrap::{
    bootstrap_wait_event_metadata, plan_bootstrap_mixed_outputs, resolve_bootstrap_wait_poll,
    BootstrapCoin, BootstrapPlanOutcome, BootstrapWaitContext, BootstrapWaitPoll,
    BootstrapWaitResolution, BootstrapWaitStepKind,
};

use super::planning::bootstrap_coins_in_base_units;
use super::BootstrapShapeContext;

/// Sleep pacing for [`wait_for_bootstrap_shape_step`] (same pattern as
/// [`crate::daemon::coinset_ws::OnceCaptureTimings`]).
#[derive(Debug, Clone, Copy)]
pub(super) struct BootstrapWaitTimings {
    pub initial_sleep_seconds: f64,
    pub max_sleep_seconds: f64,
}

impl BootstrapWaitTimings {
    pub const PRODUCTION: Self = Self {
        initial_sleep_seconds: 2.0,
        max_sleep_seconds: 20.0,
    };

    #[cfg(test)]
    pub const UNIT_TEST: Self = Self {
        initial_sleep_seconds: 0.001,
        max_sleep_seconds: 0.002,
    };
}

fn normalized_spendable_snapshot(spendable: &[BootstrapCoin]) -> Vec<BootstrapCoin> {
    let mut snapshot = spendable.to_vec();
    snapshot.sort_by(|left, right| left.id.cmp(&right.id));
    snapshot
}

async fn fetch_bootstrap_spendable(
    network: &str,
    signer: &SignerConfig,
    ctx: &BootstrapShapeContext,
) -> SignerResult<Vec<BootstrapCoin>> {
    let coins = list_wallet_unspent_coins_for_signer(
        network,
        signer,
        &ctx.receive_address,
        &ctx.split_asset_id,
    )
    .await?;
    Ok(bootstrap_coins_in_base_units(
        &coins,
        ctx.split_asset_mojo_multiplier,
    ))
}

#[derive(Debug)]
pub(super) struct BootstrapShapeStepWaitResult {
    pub events: Vec<Value>,
    pub outcome: BootstrapPlanOutcome,
    pub spendable_coins: Vec<BootstrapCoin>,
}

pub(super) struct BootstrapWaitConfig<'a> {
    pub network: &'a str,
    pub signer: &'a SignerConfig,
    pub ctx: &'a BootstrapShapeContext,
    pub timeout: Duration,
    pub step: BootstrapWaitStepKind,
    pub timings: BootstrapWaitTimings,
}

pub(super) async fn wait_for_bootstrap_shape_step(
    config: BootstrapWaitConfig<'_>,
) -> SignerResult<BootstrapShapeStepWaitResult> {
    let BootstrapWaitConfig {
        network,
        signer,
        ctx,
        timeout,
        step,
        timings,
    } = config;
    let start = Instant::now();
    // Second-granularity schedule bound for [`poll_exponential_sleep_now`]; wall-clock
    // deadline is [`timeout`] (supports sub-second unit-test deadlines).
    let schedule_timeout = i64::try_from(timeout.as_secs().max(1)).map_err(|_| {
        SignerError::Other("offer bootstrap wait timeout seconds overflow".to_string())
    })?;
    let initial_sleep = timings.initial_sleep_seconds;
    let max_sleep = timings.max_sleep_seconds;
    let mut sleep_seconds = 0.0f64;
    let mut baseline_spendable: Option<Vec<BootstrapCoin>> = None;
    loop {
        if start.elapsed() >= timeout {
            return Err(SignerError::BootstrapShapeWaitTimeout);
        }
        let elapsed_seconds = i64::try_from(start.elapsed().as_secs()).map_err(|_| {
            SignerError::Other("confirmation wait elapsed seconds overflow".to_string())
        })?;
        let Some(next_sleep) = poll_exponential_sleep_now(
            elapsed_seconds,
            schedule_timeout,
            sleep_seconds,
            initial_sleep,
            max_sleep,
        ) else {
            return Err(SignerError::BootstrapShapeWaitTimeout);
        };
        let spendable = fetch_bootstrap_spendable(network, signer, ctx).await?;
        let snapshot = normalized_spendable_snapshot(&spendable);
        let observed_on_chain_update = if let Some(baseline) = baseline_spendable.as_ref() {
            snapshot != *baseline
        } else {
            baseline_spendable = Some(snapshot);
            false
        };
        let outcome = plan_bootstrap_mixed_outputs(
            &ctx.ladder_entries,
            &spendable,
            resolve_combine_input_cap(),
            &ctx.combine_context,
        );
        if let BootstrapWaitResolution::Complete(completed) = resolve_bootstrap_wait_poll(
            match step {
                BootstrapWaitStepKind::AfterCombine => {
                    BootstrapWaitPoll::AfterCombine(BootstrapWaitContext {
                        combine_target_amount: ctx.bootstrap_plan.total_output_amount,
                        ladder_entries: &ctx.ladder_entries,
                        spendable_coins: &spendable,
                    })
                }
                BootstrapWaitStepKind::AfterSplit => BootstrapWaitPoll::AfterSplit,
            },
            &outcome,
            observed_on_chain_update,
        ) {
            let (ready, reason) = bootstrap_wait_event_metadata(step, &completed);
            return Ok(BootstrapShapeStepWaitResult {
                events: vec![json!({
                    "event": "bootstrap_shape_wait_complete",
                    "wait_step": match step {
                        BootstrapWaitStepKind::AfterCombine => "after_combine",
                        BootstrapWaitStepKind::AfterSplit => "after_split",
                    },
                    "ready": ready,
                    "reason": reason,
                    "elapsed_seconds": elapsed_seconds.to_string(),
                })],
                outcome: completed,
                spendable_coins: spendable,
            });
        }
        tokio::time::sleep(Duration::from_secs_f64(next_sleep)).await;
        sleep_seconds =
            poll_exponential_advance_sleep(sleep_seconds, initial_sleep, max_sleep, 1.5);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{wait_for_bootstrap_shape_step, BootstrapWaitConfig, BootstrapWaitTimings};
    use crate::error::SignerError;
    use crate::offer::bootstrap::{
        BaseUnits, BootstrapFundingSource, BootstrapPlan, BootstrapWaitStepKind, PlannerLadderRow,
    };
    use crate::offer::operator::BootstrapShapeContext;
    use crate::test_support::bootstrap_shape::{
        coin_record_body, coin_records_response, eco181_cap_combine_shape_context,
        BOOTSTRAP_TEST_MOJO_MULTIPLIER, BOOTSTRAP_TEST_MOJO_PER_UNIT, BOOTSTRAP_TEST_RECEIVE,
    };
    use crate::test_support::signer_config::test_signer_config;

    fn unit_test_wait_config<'a>(
        network: &'a str,
        signer: &'a crate::config::SignerConfig,
        ctx: &'a BootstrapShapeContext,
        timeout: Duration,
        step: BootstrapWaitStepKind,
    ) -> BootstrapWaitConfig<'a> {
        BootstrapWaitConfig {
            network,
            signer,
            ctx,
            timeout,
            step,
            timings: BootstrapWaitTimings::UNIT_TEST,
        }
    }

    #[tokio::test]
    async fn wait_after_combine_ignores_change_before_target_coin() {
        let change_only = coin_records_response(&[coin_record_body(
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            BOOTSTRAP_TEST_MOJO_PER_UNIT * 5,
        )]);
        let combined = coin_records_response(&[coin_record_body(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            BOOTSTRAP_TEST_MOJO_PER_UNIT * 100,
        )]);
        let mut server = mockito::Server::new_async().await;
        let _change = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(change_only)
            .expect_at_least(1)
            .create_async()
            .await;
        let _combined = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(combined)
            .expect_at_least(1)
            .create_async()
            .await;
        let signer = test_signer_config(&server.url());
        let ladder = vec![PlannerLadderRow {
            size_base_units: 100,
            target_count: 1,
            split_buffer_count: 0,
        }];
        let ctx = eco181_cap_combine_shape_context(ladder);

        let completed = wait_for_bootstrap_shape_step(unit_test_wait_config(
            "mainnet",
            &signer,
            &ctx,
            Duration::from_secs(30),
            BootstrapWaitStepKind::AfterCombine,
        ))
        .await
        .expect("combine wait should ignore change-only inventory");

        assert_eq!(
            completed.events[0]["event"],
            "bootstrap_shape_wait_complete"
        );
        assert_eq!(completed.events[0]["wait_step"], "after_combine");
        if completed.events[0]["ready"] == serde_json::json!(true) {
            assert_eq!(completed.events[0]["reason"], "bootstrap_submitted");
        } else {
            assert_eq!(completed.events[0]["reason"], "combine_step_complete");
        }
    }

    #[tokio::test]
    async fn wait_times_out_when_planner_never_satisfied() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[]}"#)
            .expect_at_least(1)
            .create_async()
            .await;
        let signer = test_signer_config(&server.url());
        let ladder = vec![PlannerLadderRow {
            size_base_units: 100,
            target_count: 1,
            split_buffer_count: 0,
        }];
        let ctx = eco181_cap_combine_shape_context(ladder);

        let err = wait_for_bootstrap_shape_step(unit_test_wait_config(
            "mainnet",
            &signer,
            &ctx,
            Duration::from_millis(20),
            BootstrapWaitStepKind::AfterCombine,
        ))
        .await
        .expect_err("timeout");
        assert!(matches!(err, SignerError::BootstrapShapeWaitTimeout));
    }

    #[tokio::test]
    async fn wait_after_split_completes_on_terminal_still_needs_split() {
        let before_split = coin_records_response(&[coin_record_body(
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            BOOTSTRAP_TEST_MOJO_PER_UNIT * 100,
        )]);
        let after_split = coin_records_response(&[coin_record_body(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            BOOTSTRAP_TEST_MOJO_PER_UNIT * 100,
        )]);
        let mut server = mockito::Server::new_async().await;
        let _before = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(before_split)
            .expect_at_least(1)
            .create_async()
            .await;
        let _after = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(after_split)
            .expect_at_least(1)
            .create_async()
            .await;
        let signer = test_signer_config(&server.url());
        let ladder = vec![PlannerLadderRow {
            size_base_units: 100,
            target_count: 2,
            split_buffer_count: 0,
        }];
        let ctx = BootstrapShapeContext {
            split_asset_id: "xch".to_string(),
            split_asset_mojo_multiplier: BOOTSTRAP_TEST_MOJO_MULTIPLIER,
            receive_address: BOOTSTRAP_TEST_RECEIVE.to_string(),
            bootstrap_plan: BootstrapPlan {
                funding: BootstrapFundingSource::SingleCoin {
                    coin_id: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                        .to_string(),
                    amount: BaseUnits::new(100),
                },
                output_amounts_base_units: vec![100],
                total_output_amount: 100,
                change_amount: 0,
                deficits: vec![crate::offer::bootstrap::LadderDeficit::new(100, 2, 1)],
            },
            ladder_entries: ladder,
            combine_context: crate::offer::bootstrap::BootstrapCombineContext::for_tests(),
            fee_mojos: 0,
            fee_source: String::new(),
            fee_lookup_error: None,
            #[cfg(test)]
            test_overrides: crate::offer::operator::SignerDenominationTestOverrides::default(),
        };

        let completed = wait_for_bootstrap_shape_step(unit_test_wait_config(
            "mainnet",
            &signer,
            &ctx,
            Duration::from_secs(30),
            BootstrapWaitStepKind::AfterSplit,
        ))
        .await
        .expect("split wait should finish on terminal still_needs_split");

        assert_eq!(completed.events[0]["wait_step"], "after_split");
        assert_eq!(
            completed.events[0]["event"],
            "bootstrap_shape_wait_complete"
        );
        assert_eq!(completed.events[0]["ready"], false);
        let reason = completed.events[0]["reason"].as_str().expect("reason");
        assert!(
            reason.contains("still_needs_split") || reason.contains("still_underfunded"),
            "reason={reason}"
        );
    }
}
