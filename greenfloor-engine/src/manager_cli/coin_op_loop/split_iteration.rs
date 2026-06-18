use serde_json::{json, Value};

use crate::coin_ops::execution::{submit_combine_prereq, CoinOpExecContext};
use crate::coin_ops::{
    plan_auto_split_selection, SpendableCoin, SplitAutoSelectPlan, SplitPlanningProfile,
};
use crate::error::SignerResult;

use super::context::{enforce_split_lockup_guardrail, COIN_SPLIT_NO_SPENDABLE_ERROR};
use super::until_ready::LoopIterationOutcome;

pub(super) struct SplitIterationParams<'a> {
    pub ctx: &'a CoinOpExecContext,
    pub iteration: i32,
    pub spendable: Vec<SpendableCoin>,
    pub gate_json: Option<Value>,
    pub explicit_coin_ids: bool,
    pub coin_ids: &'a [String],
    pub required_amount: i64,
    pub output_amounts: &'a [u64],
    pub split_fee: u64,
    pub no_wait: bool,
    pub allow_lock_all_spendable: bool,
}

pub(super) async fn run_split_iteration(
    params: SplitIterationParams<'_>,
) -> SignerResult<LoopIterationOutcome> {
    let SplitIterationParams {
        ctx,
        iteration,
        spendable,
        gate_json,
        explicit_coin_ids,
        coin_ids,
        required_amount,
        output_amounts,
        split_fee,
        no_wait,
        allow_lock_all_spendable,
    } = params;
    let selected_coin_ids = if explicit_coin_ids {
        coin_ids.to_vec()
    } else if spendable.is_empty() {
        return Ok(LoopIterationOutcome::Exit {
            code: 2,
            payload: Some(json!({"error": COIN_SPLIT_NO_SPENDABLE_ERROR})),
        });
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
                return Ok(LoopIterationOutcome::Exit {
                    code: 2,
                    payload: Some(json!({"error": skip.reason})),
                });
            }
            SplitAutoSelectPlan::Coin(plan) => vec![plan.coin_id],
        }
    };

    if let Some((code, payload)) = enforce_split_lockup_guardrail(
        &spendable,
        &selected_coin_ids,
        allow_lock_all_spendable,
        &ctx.resolved_base_asset_id,
    )? {
        return Ok(LoopIterationOutcome::Exit {
            code,
            payload: Some(payload),
        });
    }

    let operation_id = ctx
        .execute_mixed_split(output_amounts.to_vec(), &selected_coin_ids, split_fee)
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
