//! Shared coin-op loop preparation for split and combine CLI commands.

use crate::coin_ops::execution::CoinOpExecContext;
use crate::config::LadderEntry;
use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::ladder::sell_ladder_entry_for_size;

use super::loop_common::validate_until_ready_mode;
use super::until_ready::UntilReadyWaitMode;
use crate::config::{GatedOperatorMarketLoadRequest, OperatorMarketCommand};

pub(super) struct CoinOpLoopCommon<'a> {
    pub exec_ctx: CoinOpExecContext,
    pub wait: UntilReadyWaitMode,
    pub explicit_coin_ids: bool,
    pub resolved_asset_id: String,
    pub ladder_entry: Option<LadderEntry>,
    pub coin_ids: &'a [String],
}

pub(super) struct CoinOpLoopPrep<'a> {
    pub mgr: &'a ManagerContext,
    pub network: &'a str,
    pub market_id: Option<&'a str>,
    pub pair: Option<&'a str>,
    pub asset_id: Option<&'a str>,
    pub wait: UntilReadyWaitMode,
    pub size_base_units: Option<i64>,
    pub coin_ids: &'a [String],
}

pub(super) async fn prepare_coin_op_loop_common(
    prep: CoinOpLoopPrep<'_>,
) -> SignerResult<CoinOpLoopCommon<'_>> {
    validate_until_ready_mode(prep.wait, prep.size_base_units)?;
    let exec_ctx = super::context::build_coin_op_exec_context(
        &GatedOperatorMarketLoadRequest {
            program_path: &prep.mgr.program_config,
            markets_path: &prep.mgr.markets_config,
            testnet_markets_path: prep.mgr.testnet_markets_path(),
            cats_path: Some(&prep.mgr.cats_config),
            network: prep.network,
            market_id: prep.market_id,
            pair: prep.pair,
            command: OperatorMarketCommand::Build,
        },
        prep.asset_id,
        prep.mgr.state_db_override(),
    )
    .await?;
    let ladder_entry = prep
        .size_base_units
        .filter(|value| *value > 0)
        .map(|size| sell_ladder_entry_for_size(&exec_ctx.gated.market_row, size))
        .transpose()?
        .cloned();
    Ok(CoinOpLoopCommon {
        explicit_coin_ids: !prep.coin_ids.is_empty(),
        resolved_asset_id: exec_ctx.resolved_base_asset_id.clone(),
        exec_ctx,
        wait: prep.wait,
        ladder_entry,
        coin_ids: prep.coin_ids,
    })
}
