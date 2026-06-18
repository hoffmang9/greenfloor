use serde_json::{json, Value};
use std::collections::HashSet;

use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::{
    combine_output_amounts, plan_auto_combine_inputs, total_for_coin_ids,
    CombineInputSelectionMode, SpendableCoin,
};
use crate::error::{SignerError, SignerResult};

use super::until_ready::LoopIterationOutcome;

pub(super) struct CombineIterationParams<'a> {
    pub ctx: &'a CoinOpExecContext,
    pub iteration: i32,
    pub spendable: Vec<SpendableCoin>,
    pub gate_json: Option<Value>,
    pub number_of_coins: i64,
    pub target_coin_amount_mojos: i64,
    pub coin_ids: &'a [String],
    pub combine_fee: u64,
    pub no_wait: bool,
}

pub(super) async fn run_combine_iteration(
    params: CombineIterationParams<'_>,
) -> SignerResult<LoopIterationOutcome> {
    let CombineIterationParams {
        ctx,
        iteration,
        spendable,
        gate_json,
        number_of_coins,
        target_coin_amount_mojos,
        coin_ids,
        combine_fee,
        no_wait,
    } = params;
    let input_coin_ids = if coin_ids.is_empty() {
        plan_auto_combine_inputs(
            &spendable,
            number_of_coins.try_into().unwrap_or(0usize),
            CombineInputSelectionMode::ExactAmount,
            if target_coin_amount_mojos > 0 {
                Some(target_coin_amount_mojos)
            } else {
                None
            },
            None::<&HashSet<String>>,
            Some(ctx.combine_input_cap.try_into().unwrap_or(0usize)),
        )
        .map_err(|reason| SignerError::Other(reason.to_string()))?
    } else {
        coin_ids.to_vec()
    };
    if input_coin_ids.len() < 2 {
        return Ok(LoopIterationOutcome::Exit {
            code: 2,
            payload: Some(json!({"error": "insufficient_combine_inputs"})),
        });
    }

    let total = total_for_coin_ids(&spendable, &input_coin_ids);
    let output_amounts = combine_output_amounts(total, 1);
    let operation_id = ctx
        .execute_mixed_split(output_amounts, &input_coin_ids, combine_fee)
        .await?;
    Ok(LoopIterationOutcome::Continue {
        operation: json!({
            "iteration": iteration,
            "signature_request_id": operation_id,
            "input_coin_ids": input_coin_ids,
            "waited": !no_wait,
            "denomination_readiness": gate_json,
        }),
    })
}
