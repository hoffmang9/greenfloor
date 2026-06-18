use serde_json::{json, Value};

use crate::coin_ops::{
    plan_auto_split_selection, SplitAutoSelectPlan, SplitPlanningProfile, SpendableCoin,
};
use crate::coin_ops::execution::{submit_combine_prereq, CoinOpExecContext};
use crate::error::SignerResult;

use super::context::{enforce_split_lockup_guardrail, COIN_SPLIT_NO_SPENDABLE_ERROR};
use super::until_ready::LoopIterationOutcome;
use crate::manager_cli::json::emit_json;

pub(super) async fn run_split_iteration(
    ctx: &CoinOpExecContext,
    iteration: i32,
    spendable: Vec<SpendableCoin>,
    gate_json: Option<Value>,
    explicit_coin_ids: bool,
    coin_ids: &[String],
    required_amount: i64,
    output_amounts: &[u64],
    split_fee: u64,
    no_wait: bool,
    allow_lock_all_spendable: bool,
) -> SignerResult<LoopIterationOutcome> {
    let selected_coin_ids = if explicit_coin_ids {
        coin_ids.to_vec()
    } else if spendable.is_empty() {
        emit_json(&json!({"error": COIN_SPLIT_NO_SPENDABLE_ERROR}))?;
        return Ok(LoopIterationOutcome::Exit(2));
    } else {
        match plan_auto_split_selection(
            &spendable,
            required_amount,
            ctx.market.base_asset.trim(),
            SplitPlanningProfile::CliAuto,
            ctx.combine_input_cap,
            Some(iteration == 1),
        ) {
            SplitAutoSelectPlan::CombinePrereq(prereq) => {
                let operation_id = submit_combine_prereq(ctx, &prereq.input_coin_ids).await?;
                let operation = json!({
                    "iteration": iteration,
                    "op": "combine-prereq",
                    "signature_request_id": operation_id,
                    "input_coin_ids": prereq.input_coin_ids,
                    "waited": !no_wait,
                });
                if no_wait {
                    return Ok(LoopIterationOutcome::Break {
                        operation: Some(operation),
                        reason: "combine_prereq_submitted".to_string(),
                    });
                }
                return Ok(LoopIterationOutcome::Continue { operation });
            }
            SplitAutoSelectPlan::Skip(skip) => {
                emit_json(&json!({"error": skip.reason}))?;
                return Ok(LoopIterationOutcome::Exit(2));
            }
            SplitAutoSelectPlan::Coin(plan) => vec![plan.coin_id],
        }
    };

    if let Some(code) = enforce_split_lockup_guardrail(
        &spendable,
        &selected_coin_ids,
        allow_lock_all_spendable,
        &ctx.resolved_base_asset_id,
    )? {
        return Ok(LoopIterationOutcome::Exit(code));
    }

    let operation_id = ctx
        .execute_mixed_split(
            output_amounts.to_vec(),
            &selected_coin_ids,
            split_fee,
        )
        .await?;
    Ok(LoopIterationOutcome::Continue {
        operation: json!({
            "iteration": iteration,
            "signature_request_id": operation_id,
            "selected_coin_ids": selected_coin_ids,
            "waited": !no_wait,
            "denomination_readiness": gate_json,
        }),
    })
}
