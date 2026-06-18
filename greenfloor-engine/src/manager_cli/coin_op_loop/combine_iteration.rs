use serde_json::{json, Value};

use crate::coin_ops::{
    combine_output_amounts, plan_auto_combine_inputs, total_for_coin_ids,
    CombineInputSelectionMode, SpendableCoin,
};
use crate::coin_ops::execution::CoinOpExecContext;
use crate::error::{SignerError, SignerResult};

use super::until_ready::LoopIterationOutcome;
use crate::manager_cli::json::emit_json;

pub(super) async fn run_combine_iteration(
    ctx: &CoinOpExecContext,
    iteration: i32,
    spendable: Vec<SpendableCoin>,
    gate_json: Option<Value>,
    number_of_coins: i64,
    target_coin_amount_mojos: i64,
    coin_ids: &[String],
    combine_fee: u64,
    no_wait: bool,
) -> SignerResult<LoopIterationOutcome> {
    let input_coin_ids = if coin_ids.is_empty() {
        plan_auto_combine_inputs(
            &spendable,
            number_of_coins as usize,
            CombineInputSelectionMode::ExactAmount,
            if target_coin_amount_mojos > 0 {
                Some(target_coin_amount_mojos)
            } else {
                None
            },
            None,
            Some(ctx.combine_input_cap as usize),
        )
        .map_err(|reason| SignerError::Other(reason.to_string()))?
    } else {
        coin_ids.to_vec()
    };
    if input_coin_ids.len() < 2 {
        emit_json(&json!({"error": "insufficient_combine_inputs"}))?;
        return Ok(LoopIterationOutcome::Exit(2));
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
