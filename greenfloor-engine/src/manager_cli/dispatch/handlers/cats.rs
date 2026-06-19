//! CAT registry dispatch helpers.

use crate::error::SignerResult;
use crate::manager_cli::cats::{self, CatsAddRequest};
use crate::manager_cli::context::ManagerContext;

#[allow(clippy::too_many_arguments)]
pub async fn run_cats_add(
    ctx: &ManagerContext,
    network: String,
    cat_id: Option<String>,
    ticker: Option<String>,
    name: Option<String>,
    base_symbol: Option<String>,
    ticker_id: Option<String>,
    pool_id: Option<String>,
    last_price_xch: Option<String>,
    target_usd_per_unit: Option<String>,
    no_dexie_lookup: bool,
    replace: bool,
) -> SignerResult<i32> {
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

pub fn run_cats_list(ctx: &ManagerContext) -> SignerResult<i32> {
    cats::run_cats_list(ctx)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_cats_delete(
    ctx: &ManagerContext,
    network: String,
    cat_id: Option<String>,
    ticker: Option<String>,
    no_dexie_lookup: bool,
    yes: bool,
    preflight_only: bool,
) -> SignerResult<i32> {
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
