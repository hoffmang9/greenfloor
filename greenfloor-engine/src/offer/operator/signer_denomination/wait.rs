use serde_json::{json, Value};

use crate::coin_ops::execution::resolve_combine_input_cap;
use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::SignerConfig;
use crate::cycle::retry::{poll_exponential_advance_sleep, poll_exponential_sleep_now};
use crate::error::{SignerError, SignerResult};
use crate::offer::bootstrap::{
    bootstrap_wait_step_satisfied, plan_bootstrap_mixed_outputs, BootstrapWaitStepKind,
};

use super::planning::bootstrap_coins_in_base_units;
use super::BootstrapShapeContext;

pub(super) struct BootstrapWaitConfig<'a> {
    pub network: &'a str,
    pub signer: &'a SignerConfig,
    pub ctx: &'a BootstrapShapeContext,
    pub timeout_seconds: u64,
    pub min_timeout_seconds: u64,
    pub step: BootstrapWaitStepKind,
}

pub(super) async fn wait_for_bootstrap_shape_ready(
    config: BootstrapWaitConfig<'_>,
) -> SignerResult<Vec<Value>> {
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
            return Err(SignerError::Other("confirmation_wait_timeout".to_string()));
        };
        let coins = list_wallet_unspent_coins_for_signer(
            network,
            signer,
            &ctx.receive_address,
            &ctx.split_asset_id,
        )
        .await?;
        let spendable = bootstrap_coins_in_base_units(&coins, ctx.split_asset_mojo_multiplier);
        let outcome = plan_bootstrap_mixed_outputs(
            &ctx.ladder_entries,
            &spendable,
            resolve_combine_input_cap(),
            &ctx.combine_context,
        );
        if bootstrap_wait_step_satisfied(step, &outcome) {
            return Ok(vec![json!({
                "event": "bootstrap_shape_ready",
                "wait_step": match step {
                    BootstrapWaitStepKind::AfterCombine => "after_combine",
                    BootstrapWaitStepKind::AfterSplit => "after_split",
                },
                "elapsed_seconds": elapsed_seconds.to_string(),
            })]);
        }
        tokio::time::sleep(std::time::Duration::from_secs_f64(next_sleep)).await;
        sleep_seconds =
            poll_exponential_advance_sleep(sleep_seconds, initial_sleep, max_sleep, 1.5);
    }
}

#[cfg(test)]
mod tests {
    use super::{wait_for_bootstrap_shape_ready, BootstrapWaitConfig};
    use crate::offer::bootstrap::{BootstrapWaitStepKind, PlannerLadderRow};
    use crate::test_support::bootstrap_shape::{
        coin_record_body, coin_records_response, eco181_cap_combine_shape_context,
        BOOTSTRAP_TEST_MOJO_PER_UNIT,
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

        let events = wait_for_bootstrap_shape_ready(BootstrapWaitConfig {
            network: "mainnet",
            signer: &signer,
            ctx: &ctx,
            timeout_seconds: 30,
            min_timeout_seconds: TEST_MIN_TIMEOUT_SECONDS,
            step: BootstrapWaitStepKind::AfterCombine,
        })
        .await
        .expect("combine wait should ignore change-only inventory");

        assert_eq!(events[0]["event"], "bootstrap_shape_ready");
        assert_eq!(events[0]["wait_step"], "after_combine");
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

        let err = wait_for_bootstrap_shape_ready(BootstrapWaitConfig {
            network: "mainnet",
            signer: &signer,
            ctx: &ctx,
            timeout_seconds: 1,
            min_timeout_seconds: TEST_MIN_TIMEOUT_SECONDS,
            step: BootstrapWaitStepKind::AfterCombine,
        })
        .await
        .expect_err("timeout");
        assert_eq!(err.to_string(), "confirmation_wait_timeout");
    }
}
