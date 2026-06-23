use std::collections::HashSet;

use serde_json::json;

use crate::coin_ops::execution::resolve_combine_input_cap;
use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::{ManagerProgramConfig, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapPlan, BootstrapPlanOutcome,
    PlannerLadderRow,
};

use super::planning::bootstrap_coins_in_base_units;
use super::split_submit::{submit_bootstrap_combine, submit_bootstrap_mixed_split};
use super::wait::{wait_for_coinset_confirmation, BootstrapWaitConfig};
use super::{
    executed_after_split, BootstrapPhaseFailure, BootstrapPhaseResult, ExecutedAfterSplitParams,
};

const BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS: u64 = 10;

pub(crate) struct BootstrapShapeContext {
    pub(crate) split_asset_id: String,
    pub(crate) split_asset_mojo_multiplier: i64,
    pub(crate) receive_address: String,
    pub(crate) bootstrap_plan: BootstrapPlan,
    pub(crate) ladder_entries: Vec<PlannerLadderRow>,
    pub(crate) fee_mojos: u64,
    pub(crate) fee_source: String,
    pub(crate) fee_lookup_error: Option<String>,
    pub(crate) existing_coin_ids: HashSet<String>,
}

fn bootstrap_failed(failure: BootstrapPhaseFailure) -> BootstrapPhaseResult {
    BootstrapPhaseResult::failed(failure)
}

fn bootstrap_result_from_replan(
    replanned: &BootstrapPlanOutcome,
    ctx: &BootstrapShapeContext,
    prepend_wait_events: Vec<serde_json::Value>,
) -> BootstrapPhaseResult {
    let executed = bootstrap_executed_phase(replanned);
    let mut result = BootstrapPhaseResult::from_snapshot(executed);
    result.fee_mojos = ctx.fee_mojos;
    result.fee_source.clone_from(&ctx.fee_source);
    result.fee_lookup_error.clone_from(&ctx.fee_lookup_error);
    result.wait_events = prepend_wait_events;
    result
}

async fn wait_for_bootstrap_shape_confirmation(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &BootstrapShapeContext,
    failure_reason: &'static str,
) -> Result<Vec<serde_json::Value>, BootstrapPhaseResult> {
    wait_for_coinset_confirmation(BootstrapWaitConfig {
        network: &program.network,
        signer: signer_config,
        receive_address: &ctx.receive_address,
        asset_id: &ctx.split_asset_id,
        initial_coin_ids: &ctx.existing_coin_ids,
        timeout_seconds: program.runtime_offer_bootstrap_wait_timeout_seconds,
        min_timeout_seconds: BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS,
    })
    .await
    .map_err(|err| {
        bootstrap_failed(
            BootstrapPhaseFailure::new(
                failure_reason,
                ctx.fee_mojos,
                ctx.fee_source.clone(),
                ctx.fee_lookup_error.clone(),
            )
            .with_plan(ctx.bootstrap_plan.clone())
            .with_wait_error(err.to_string()),
        )
    })
}

async fn refresh_bootstrap_spendable(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &BootstrapShapeContext,
) -> SignerResult<(
    Vec<crate::coinset::WalletUnspentCoin>,
    Vec<crate::offer::bootstrap::BootstrapCoin>,
)> {
    let asset_coins = list_wallet_unspent_coins_for_signer(
        &program.network,
        signer_config,
        &ctx.receive_address,
        &ctx.split_asset_id,
    )
    .await?;
    let spendable = bootstrap_coins_in_base_units(&asset_coins, ctx.split_asset_mojo_multiplier);
    Ok((asset_coins, spendable))
}

async fn execute_bootstrap_combine_step(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &BootstrapShapeContext,
) -> Result<Vec<serde_json::Value>, BootstrapPhaseResult> {
    let combine_result = submit_bootstrap_combine(
        signer_config,
        &ctx.bootstrap_plan,
        &ctx.split_asset_id,
        &ctx.receive_address,
        ctx.split_asset_mojo_multiplier,
    )
    .await
    .map_err(|err| {
        bootstrap_failed(BootstrapPhaseFailure::new(
            format!("signer_bootstrap_combine_error:{err}"),
            ctx.fee_mojos,
            ctx.fee_source.clone(),
            ctx.fee_lookup_error.clone(),
        ))
    })?;

    let mut wait_events = wait_for_bootstrap_shape_confirmation(
        program,
        signer_config,
        ctx,
        "bootstrap_combine_wait_failed",
    )
    .await?;
    wait_events.insert(
        0,
        json!({
            "event": "bootstrap_combine_submitted",
            "combine_result": combine_result,
        }),
    );
    Ok(wait_events)
}

async fn replan_after_combine(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &mut BootstrapShapeContext,
    prepend_wait_events: Vec<serde_json::Value>,
) -> SignerResult<Option<BootstrapPhaseResult>> {
    let (refreshed_asset_coins, refreshed_spendable) =
        refresh_bootstrap_spendable(program, signer_config, ctx).await?;
    ctx.existing_coin_ids = refreshed_asset_coins
        .iter()
        .map(|coin| coin.id.clone())
        .collect();

    let replanned = plan_bootstrap_mixed_outputs(
        &ctx.ladder_entries,
        &refreshed_spendable,
        resolve_combine_input_cap(),
    );
    let BootstrapPlanOutcome::NeedsShape(split_plan) = replanned else {
        return Ok(Some(bootstrap_result_from_replan(
            &replanned,
            ctx,
            prepend_wait_events,
        )));
    };
    if split_plan.requires_combine_first() {
        return Ok(Some(bootstrap_result_from_replan(
            &BootstrapPlanOutcome::NeedsShape(split_plan),
            ctx,
            prepend_wait_events,
        )));
    }
    ctx.bootstrap_plan = split_plan;
    Ok(None)
}

pub(super) async fn execute_bootstrap_shape(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    mut ctx: BootstrapShapeContext,
) -> SignerResult<BootstrapPhaseResult> {
    let mut prepend_wait_events = Vec::new();

    if ctx.bootstrap_plan.requires_combine_first() {
        prepend_wait_events =
            match execute_bootstrap_combine_step(program, signer_config, &ctx).await {
                Ok(events) => events,
                Err(result) => return Ok(result),
            };
        if let Some(result) = replan_after_combine(
            program,
            signer_config,
            &mut ctx,
            prepend_wait_events.clone(),
        )
        .await?
        {
            return Ok(result);
        }
    }

    let bootstrap_plan = ctx.bootstrap_plan.clone();
    let split_result = match submit_bootstrap_mixed_split(
        signer_config,
        &bootstrap_plan,
        &ctx.split_asset_id,
        &ctx.receive_address,
        ctx.split_asset_mojo_multiplier,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok(bootstrap_failed(
                BootstrapPhaseFailure::new(
                    format!("signer_mixed_split_error:{err}"),
                    ctx.fee_mojos,
                    ctx.fee_source.clone(),
                    ctx.fee_lookup_error.clone(),
                )
                .with_plan(bootstrap_plan),
            ));
        }
    };

    let mut wait_events = match wait_for_bootstrap_shape_confirmation(
        program,
        signer_config,
        &ctx,
        "bootstrap_wait_failed",
    )
    .await
    {
        Ok(events) => events,
        Err(mut failure) => {
            failure.split_result = split_result;
            return Ok(failure);
        }
    };
    wait_events.splice(0..0, prepend_wait_events);

    let (_, refreshed_spendable) =
        refresh_bootstrap_spendable(program, signer_config, &ctx).await?;
    Ok(executed_after_split(ExecutedAfterSplitParams {
        fee_mojos: ctx.fee_mojos,
        fee_source: ctx.fee_source,
        fee_lookup_error: ctx.fee_lookup_error,
        split_result,
        wait_events,
        bootstrap_plan,
        ladder_entries: &ctx.ladder_entries,
        refreshed_spendable: &refreshed_spendable,
    }))
}

#[cfg(test)]
mod tests {
    use crate::config::ManagerProgramConfig;
    use crate::offer::bootstrap::{
        plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow,
    };
    use crate::test_support::signer_config::test_signer_config;

    use super::{replan_after_combine, BootstrapShapeContext};

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
    const MOJO_PER_UNIT: u64 = 1_000;
    const MOJO_PER_XCH: u64 = 1_000_000_000_000;

    fn combine_first_shape_context(
        receive_address: &str,
        split_asset_id: &str,
        ladder: Vec<PlannerLadderRow>,
    ) -> BootstrapShapeContext {
        let spendable = vec![
            BootstrapCoin {
                id: "sixty-five".to_string(),
                amount: 65,
            },
            BootstrapCoin {
                id: "twenty".to_string(),
                amount: 20,
            },
            BootstrapCoin {
                id: "eleven".to_string(),
                amount: 11,
            },
            BootstrapCoin {
                id: "four".to_string(),
                amount: 4,
            },
        ];
        let BootstrapPlanOutcome::NeedsShape(bootstrap_plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable, 5)
        else {
            panic!("expected combine-first plan");
        };
        BootstrapShapeContext {
            split_asset_id: split_asset_id.to_string(),
            split_asset_mojo_multiplier: 1_000,
            receive_address: receive_address.to_string(),
            bootstrap_plan,
            ladder_entries: ladder,
            fee_mojos: 0,
            fee_source: String::new(),
            fee_lookup_error: None,
            existing_coin_ids: spendable.iter().map(|coin| coin.id.clone()).collect(),
        }
    }

    fn coin_record_body(parent: &str, amount: u64) -> String {
        format!(
            r#"{{
            "coin": {{
                "parent_coin_info": "{parent}",
                "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                "amount": {amount}
            }},
            "coinbase": false,
            "confirmed_block_index": 1,
            "spent": false,
            "spent_block_index": 0,
            "timestamp": 1
        }}"#
        )
    }

    #[tokio::test]
    async fn replan_after_combine_transitions_to_single_coin_split() {
        let combined_parent = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let combined_record = coin_record_body(combined_parent, MOJO_PER_UNIT * 100);

        let mut server = mockito::Server::new_async().await;
        let combined_coin_body = format!(
            r#"{{
            "success": true,
            "coin_records": [{combined_record}]
        }}"#
        );
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(combined_coin_body)
            .create_async()
            .await;

        let program = ManagerProgramConfig::default();
        let signer = test_signer_config(&server.url());
        let ladder = vec![PlannerLadderRow {
            size_base_units: 100,
            target_count: 1,
            split_buffer_count: 0,
        }];
        let mut ctx = combine_first_shape_context(RECEIVE_ADDRESS, "xch", ladder);
        assert!(ctx.bootstrap_plan.requires_combine_first());

        let replanned = replan_after_combine(
            &program,
            &signer,
            &mut ctx,
            vec![serde_json::json!({"event": "bootstrap_combine_submitted"})],
        )
        .await
        .expect("replan");

        match replanned {
            None => {
                assert!(!ctx.bootstrap_plan.requires_combine_first());
                assert_eq!(ctx.bootstrap_plan.output_amounts_base_units, vec![100]);
            }
            Some(result) => {
                assert!(result.ready);
                assert_eq!(result.reason, "bootstrap_submitted");
            }
        }
    }

    #[tokio::test]
    async fn prepare_and_replan_combine_first_inventory() {
        use crate::offer::operator::signer_denomination::prepare_bootstrap_execution_plan;
        use crate::test_support::ladder::market_with_side_ladder;

        let mut server = mockito::Server::new_async().await;
        let fragmented = format!(
            r#"{{
            "success": true,
            "coin_records": [{}, {}, {}, {}]
        }}"#,
            coin_record_body(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                MOJO_PER_XCH * 65,
            ),
            coin_record_body(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                MOJO_PER_XCH * 20,
            ),
            coin_record_body(
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                MOJO_PER_XCH * 11,
            ),
            coin_record_body(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                MOJO_PER_XCH * 4,
            ),
        );
        let combined = format!(
            r#"{{
            "success": true,
            "coin_records": [{}]
        }}"#,
            coin_record_body(
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                MOJO_PER_XCH * 100,
            )
        );
        let _initial = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(fragmented)
            .expect_at_least(1)
            .create_async()
            .await;
        let _after_combine = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(combined)
            .expect_at_least(1)
            .create_async()
            .await;
        let _fee = server
            .mock("POST", "/get_fee_estimate")
            .with_status(200)
            .with_body(r#"{"success":false}"#)
            .create_async()
            .await;

        let mut market = market_with_side_ladder(RECEIVE_ADDRESS, "sell", 100, 1);
        market.ladders.get_mut("sell").expect("sell ladder")[0].split_buffer_count = 0;
        let program = ManagerProgramConfig {
            coin_ops_minimum_fee_mojos: 0,
            ..Default::default()
        };
        let signer = test_signer_config(&server.url());

        let mut shape_ctx =
            prepare_bootstrap_execution_plan(&program, &signer, &market, "sell", "xch", "xch", 1.0)
                .await
                .expect("plan result")
                .expect("shape context");
        assert!(shape_ctx.bootstrap_plan.requires_combine_first());

        let replanned = replan_after_combine(&program, &signer, &mut shape_ctx, Vec::new())
            .await
            .expect("replan");
        match replanned {
            None => {
                assert!(!shape_ctx.bootstrap_plan.requires_combine_first());
                assert_eq!(
                    shape_ctx.bootstrap_plan.output_amounts_base_units,
                    vec![100]
                );
            }
            Some(result) => {
                assert!(result.ready);
                assert_eq!(result.reason, "bootstrap_submitted");
            }
        }
    }
}
