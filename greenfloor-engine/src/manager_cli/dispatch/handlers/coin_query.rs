//! Coin list/status dispatch helpers.

use crate::cli_util::optional_str;
use crate::error::SignerResult;
use crate::manager_cli::coin_op_loop;
use crate::manager_cli::context::ManagerContext;

pub async fn run_coins_list(
    ctx: &ManagerContext,
    asset: String,
    vault_id: String,
    cat_id: String,
) -> SignerResult<i32> {
    coin_op_loop::run_coins_list(
        ctx,
        optional_str(&asset),
        optional_str(&vault_id),
        optional_str(&cat_id),
    )
    .await
}

pub async fn run_coin_status(
    ctx: &ManagerContext,
    asset: String,
    vault_id: String,
    cat_id: String,
) -> SignerResult<i32> {
    coin_op_loop::run_coin_status(
        ctx,
        optional_str(&asset),
        optional_str(&vault_id),
        optional_str(&cat_id),
    )
    .await
}
