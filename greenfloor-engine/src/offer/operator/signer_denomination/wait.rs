use serde_json::{json, Value};

use crate::coin_ops::execution::resolve_combine_input_cap;
use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::SignerConfig;
use crate::cycle::retry::{poll_exponential_advance_sleep, poll_exponential_sleep_now};
use crate::error::{SignerError, SignerResult};
use crate::offer::bootstrap::{
    bootstrap_wait_event_metadata, plan_bootstrap_mixed_outputs, resolve_bootstrap_wait_poll,
    BootstrapCoin, BootstrapPlanOutcome, BootstrapWaitResolution, BootstrapWaitStepKind,
};

use super::planning::bootstrap_coins_in_base_units;
use super::BootstrapShapeContext;

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
}

pub(super) struct BootstrapWaitConfig<'a> {
    pub network: &'a str,
    pub signer: &'a SignerConfig,
    pub ctx: &'a BootstrapShapeContext,
    pub timeout_seconds: u64,
    pub min_timeout_seconds: u64,
    pub step: BootstrapWaitStepKind,
}

pub(super) async fn wait_for_bootstrap_shape_step(
    config: BootstrapWaitConfig<'_>,
) -> SignerResult<BootstrapShapeStepWaitResult> {
    let BootstrapWaitConfig {
        network,
        signer,
        ctx,
        timeout_seconds,
        min_timeout_seconds,
        step,
    } = config;
    let start = std::time::Instant::now();
    let timeout = crate::config::u64_to_i64(
        timeout_seconds.max(min_timeout_seconds.max(1)),
        "runtime.offer_bootstrap_wait_timeout_seconds",
    )?;
    let initial_sleep = 2.0f64;
    let max_sleep = 20.0f64;
    let mut sleep_seconds = 0.0f64;
    let mut baseline_spendable: Option<Vec<BootstrapCoin>> = None;
    loop {
        let elapsed_seconds = i64::try_from(start.elapsed().as_secs()).map_err(|_| {
            SignerError::Other("confirmation wait elapsed seconds overflow".to_string())
        })?;
        let Some(next_sleep) = poll_exponential_sleep_now(
            elapsed_seconds,
            timeout,
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
        if let BootstrapWaitResolution::Complete(completed) =
            resolve_bootstrap_wait_poll(step, &outcome, observed_on_chain_update)
        {
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
            });
        }
        tokio::time::sleep(std::time::Duration::from_secs_f64(next_sleep)).await;
        sleep_seconds =
            poll_exponential_advance_sleep(sleep_seconds, initial_sleep, max_sleep, 1.5);
    }
}

#[cfg(test)]
mod tests {
    use super::{wait_for_bootstrap_shape_step, BootstrapWaitConfig};
    use crate::error::SignerError;
    use crate::offer::bootstrap::{BaseUnits, BootstrapCoin};
    use crate::offer::bootstrap::{BootstrapWaitStepKind, PlannerLadderRow};
    use crate::test_support::bootstrap_shape::{
        coin_record_body, coin_records_response, combine_first_shape_context,
        eco181_cap_combine_shape_context, BOOTSTRAP_TEST_MOJO_MULTIPLIER,
        BOOTSTRAP_TEST_MOJO_PER_UNIT, BOOTSTRAP_TEST_RECEIVE,
    };
    use crate::test_support::signer_config::test_signer_config;

    const TEST_MIN_TIMEOUT_SECONDS: u64 = 1;

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

        let completed = wait_for_bootstrap_shape_step(BootstrapWaitConfig {
            network: "mainnet",
            signer: &signer,
            ctx: &ctx,
            timeout_seconds: 30,
            min_timeout_seconds: TEST_MIN_TIMEOUT_SECONDS,
            step: BootstrapWaitStepKind::AfterCombine,
        })
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

        let err = wait_for_bootstrap_shape_step(BootstrapWaitConfig {
            network: "mainnet",
            signer: &signer,
            ctx: &ctx,
            timeout_seconds: 1,
            min_timeout_seconds: TEST_MIN_TIMEOUT_SECONDS,
            step: BootstrapWaitStepKind::AfterCombine,
        })
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
        let spendable = vec![BootstrapCoin {
            id: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
            amount: BaseUnits::new(100),
        }];
        let ctx = combine_first_shape_context(
            BOOTSTRAP_TEST_RECEIVE,
            "xch",
            BOOTSTRAP_TEST_MOJO_MULTIPLIER,
            ladder,
            &spendable,
            5,
        );

        let completed = wait_for_bootstrap_shape_step(BootstrapWaitConfig {
            network: "mainnet",
            signer: &signer,
            ctx: &ctx,
            timeout_seconds: 30,
            min_timeout_seconds: TEST_MIN_TIMEOUT_SECONDS,
            step: BootstrapWaitStepKind::AfterSplit,
        })
        .await
        .expect("split wait should finish on terminal still_needs_split");

        assert_eq!(completed.events[0]["wait_step"], "after_split");
        assert_eq!(
            completed.events[0]["event"],
            "bootstrap_shape_wait_complete"
        );
        assert_eq!(completed.events[0]["ready"], false);
        assert!(completed.events[0]["reason"]
            .as_str()
            .expect("reason")
            .contains("still_needs_split"));
    }
}
