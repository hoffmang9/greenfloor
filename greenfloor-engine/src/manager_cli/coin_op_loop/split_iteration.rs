use serde_json::{json, Value};

use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::{plan_cli_auto_split_selection, CliSplitSelection, SpendableCoin};
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
        match plan_cli_auto_split_selection(&spendable) {
            CliSplitSelection::Skip(reason) => {
                return Ok(LoopIterationOutcome::Exit {
                    code: 2,
                    payload: Some(json!({"error": reason.as_str()})),
                });
            }
            CliSplitSelection::Coin(plan) => vec![plan.coin_id],
        }
    };

    if let Some((code, payload)) = enforce_split_lockup_guardrail(
        &spendable,
        &selected_coin_ids,
        allow_lock_all_spendable,
        &ctx.resolved_base_asset_id,
    ) {
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
