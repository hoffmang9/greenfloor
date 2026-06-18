use crate::coin_ops::{combine_output_amounts, total_for_coin_ids};
use crate::error::SignerResult;

use super::context::CoinOpExecContext;

pub async fn submit_combine_prereq(
    ctx: &CoinOpExecContext,
    input_coin_ids: &[String],
) -> SignerResult<String> {
    let spendable = ctx.list_spendable_coins().await?;
    let total = total_for_coin_ids(&spendable, input_coin_ids);
    let output_amounts = combine_output_amounts(total, input_coin_ids.len());
    ctx.execute_mixed_split(
        output_amounts,
        input_coin_ids,
        ctx.program.coin_ops_combine_fee_mojos.max(0) as u64,
    )
    .await
}
