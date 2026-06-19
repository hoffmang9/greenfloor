use crate::error::SignerResult;
use crate::manager_cli::cats::{self, CatsAddRequest};
use crate::manager_cli::context::ManagerContext;

use super::super::clap::ManagerCommands;

pub async fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    match command {
        ManagerCommands::CatsAdd {
            network,
            cat_id,
            ticker,
            name,
            base_symbol,
            ticker_id,
            pool_id,
            last_price_xch,
            target_usd_per_unit,
            no_dexie_lookup,
            replace,
        } => {
            cats::run_cats_add(CatsAddRequest {
                ctx,
                network: &network,
                cat_id: cat_id.as_deref(),
                ticker: ticker.as_deref(),
                name: name.as_deref(),
                base_symbol: base_symbol.as_deref(),
                ticker_id: ticker_id.as_deref(),
                pool_id: pool_id.as_deref(),
                last_price_xch: last_price_xch.as_deref(),
                target_usd_per_unit: target_usd_per_unit.as_deref(),
                use_dexie_lookup: !no_dexie_lookup,
                replace,
            })
            .await
        }
        ManagerCommands::CatsList => cats::run_cats_list(ctx),
        ManagerCommands::CatsDelete {
            network,
            cat_id,
            ticker,
            no_dexie_lookup,
            yes,
            preflight_only,
        } => {
            cats::run_cats_delete(
                ctx,
                &network,
                cat_id.as_deref(),
                ticker.as_deref(),
                !no_dexie_lookup,
                yes,
                preflight_only,
            )
            .await
        }
        other => unreachable!("cats::run_command called with {other:?}"),
    }
}
