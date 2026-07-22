use serde_json::{json, Value};
use std::collections::HashSet;

use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::i64_to_usize;
use crate::coin_ops::{plan_exact_amount_combine_inputs, SpendableCoin};
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
        no_wait,
    } = params;
    let requested_count = i64_to_usize(number_of_coins, "combine.number_of_coins")?;
    let capped_count = i64_to_usize(ctx.combine_input_cap, "combine.input_cap")?;
    let input_coin_ids = if coin_ids.is_empty() {
        if target_coin_amount_mojos <= 0 {
            return Err(SignerError::Other(
                "target_coin_amount_mojos must be positive for auto combine selection".to_string(),
            ));
        }
        plan_exact_amount_combine_inputs(
            &spendable,
            requested_count,
            target_coin_amount_mojos,
            None::<&HashSet<String>>,
            Some(capped_count),
        )
    } else {
        coin_ids.to_vec()
    };
    if input_coin_ids.len() < 2 {
        return Ok(LoopIterationOutcome::Exit {
            code: 2,
            payload: Some(json!({"error": "insufficient_combine_inputs"})),
        });
    }
    ctx.reject_watched_inputs(&input_coin_ids)?;

    let operation_id = ctx
        .execute_combine(&input_coin_ids, Some(&spendable))
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
