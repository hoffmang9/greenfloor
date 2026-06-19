//! Coin list/status dispatch handlers.

use crate::cli_util::optional_str;
use crate::error::SignerResult;

use super::super::super::coin_op_loop;
use super::super::super::commands::ManagerCommands;
use super::super::super::context::ManagerContext;

pub async fn dispatch_coin_query_command(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> SignerResult<i32> {
    match command {
        ManagerCommands::CoinsList {
            asset,
            vault_id,
            cat_id,
        } => {
            Box::pin(coin_op_loop::run_coins_list(
                ctx,
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            ))
            .await
        }
        ManagerCommands::CoinStatus {
            asset,
            vault_id,
            cat_id,
        } => {
            Box::pin(coin_op_loop::run_coin_status(
                ctx,
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            ))
            .await
        }
        other => Err(crate::error::SignerError::Other(format!(
            "unexpected coin query command: {other:?}"
        ))),
    }
}
