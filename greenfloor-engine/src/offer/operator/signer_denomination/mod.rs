//! Runtime signer denomination bootstrap (vault mixed-split) for offer build/post.
//!
//! Deterministic ladder planning lives in `offer::bootstrap`; this module executes
//! the signer-side denomination phase before offer construction.

mod bootstrap_execute;
mod futures;
mod planning;
mod split_submit;
#[cfg(test)]
mod test_overrides;
mod types;
mod wait;

use std::collections::HashSet;

use crate::coin_ops::execution::resolve_combine_input_cap;
use crate::coinset::WalletUnspentCoin;
use crate::config::{ManagerProgramConfig, MarketConfig, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_early_phase, bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapCoin,
    BootstrapCombineContext, BootstrapPlanOutcome, PlannerLadderRow,
};
use crate::offer::build_context::mojo_multiplier_for_leg;
use crate::offer::request::{normalize_offer_side, signer_split_asset_id};

pub use types::BootstrapPhaseResult;

use bootstrap_execute::{execute_bootstrap_shape, BootstrapShapeContext};
use futures::SignerDenominationPhaseFuture;
use planning::{
    bootstrap_coins_in_base_units, bootstrap_ladder_entries_for_side, resolve_bootstrap_split_fee,
};
use types::BootstrapPhaseFailure;

#[cfg(test)]
fn spendable_bootstrap_coins(coins: &[WalletUnspentCoin]) -> Vec<BootstrapCoin> {
    bootstrap_coins_in_base_units(coins, 1)
}

fn bootstrap_skipped(reason: impl Into<String>) -> BootstrapPhaseResult {
    BootstrapPhaseResult::skipped(reason)
}

fn bootstrap_failed(failure: BootstrapPhaseFailure) -> BootstrapPhaseResult {
    BootstrapPhaseResult::failed(failure)
}

async fn load_asset_scoped_coins(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    receive_address: &str,
    split_asset_id: &str,
) -> Result<Vec<WalletUnspentCoin>, BootstrapPhaseResult> {
    crate::coinset::list_wallet_unspent_coins_for_signer(
        &program.network,
        signer_config,
        receive_address,
        split_asset_id,
    )
    .await
    .map_err(|err| {
        BootstrapPhaseResult::failed(BootstrapPhaseFailure::new(
            format!("bootstrap_coin_list_failed:{err}"),
            0,
            String::new(),
            None,
        ))
    })
}

pub(crate) struct ExecutedAfterSplitParams<'a> {
    pub(crate) fee_mojos: u64,
    pub(crate) fee_source: String,
    pub(crate) fee_lookup_error: Option<String>,
    pub(crate) split_result: serde_json::Value,
    pub(crate) wait_events: Vec<serde_json::Value>,
    pub(crate) bootstrap_plan: crate::offer::bootstrap::BootstrapPlan,
    pub(crate) ladder_entries: &'a [PlannerLadderRow],
    pub(crate) refreshed_spendable: &'a [BootstrapCoin],
    pub(crate) combine_context: BootstrapCombineContext,
}

pub(crate) fn executed_after_split(params: ExecutedAfterSplitParams<'_>) -> BootstrapPhaseResult {
    let ExecutedAfterSplitParams {
        fee_mojos,
        fee_source,
        fee_lookup_error,
        split_result,
        wait_events,
        bootstrap_plan,
        ladder_entries,
        refreshed_spendable,
        combine_context,
    } = params;
    let remaining = plan_bootstrap_mixed_outputs(
        ladder_entries,
        refreshed_spendable,
        resolve_combine_input_cap(),
        &combine_context,
    );
    let executed = bootstrap_executed_phase(&remaining);
    let mut result = BootstrapPhaseResult::from_snapshot(executed);
    result.fee_mojos = fee_mojos;
    result.fee_source = fee_source;
    result.fee_lookup_error = fee_lookup_error;
    result.split_result = split_result;
    result.wait_events = wait_events;
    result.plan = Some(bootstrap_plan);
    result
}

pub(crate) async fn prepare_bootstrap_execution_plan(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    market: &MarketConfig,
    action_side: &str,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    quote_price: f64,
) -> SignerResult<Result<BootstrapShapeContext, BootstrapPhaseResult>> {
    let side = normalize_offer_side(action_side);
    let side_ladder = market.ladders.get(side).cloned().unwrap_or_default();
    if side_ladder.is_empty() {
        return Ok(Err(bootstrap_skipped(format!("missing_{side}_ladder"))));
    }

    let ladder_entries = bootstrap_ladder_entries_for_side(
        side,
        &side_ladder,
        &market.pricing,
        quote_price,
        resolved_quote_asset_id,
    )?;
    if ladder_entries.is_empty() {
        return Ok(Err(bootstrap_skipped(format!(
            "empty_{side}_ladder_after_quote_conversion"
        ))));
    }

    let split_asset_id =
        signer_split_asset_id(side, resolved_base_asset_id, resolved_quote_asset_id);
    if split_asset_id.trim().is_empty() {
        return Ok(Err(bootstrap_skipped(format!(
            "missing_{side}_asset_for_bootstrap"
        ))));
    }
    let mojo_field = if side == "buy" {
        "quote_unit_mojo_multiplier"
    } else {
        "base_unit_mojo_multiplier"
    };
    let split_asset_mojo_multiplier =
        mojo_multiplier_for_leg(&market.pricing, mojo_field, &split_asset_id).max(1);

    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Ok(Err(bootstrap_skipped(
            "missing_receive_address_for_bootstrap",
        )));
    }

    let asset_scoped_coins =
        match load_asset_scoped_coins(program, signer_config, receive_address, &split_asset_id)
            .await
        {
            Ok(coins) => coins,
            Err(result) => return Ok(Err(result)),
        };

    let spendable_coins =
        bootstrap_coins_in_base_units(&asset_scoped_coins, split_asset_mojo_multiplier);
    let combine_context =
        BootstrapCombineContext::new(split_asset_mojo_multiplier, &split_asset_id);
    let outcome = plan_bootstrap_mixed_outputs(
        &ladder_entries,
        &spendable_coins,
        resolve_combine_input_cap(),
        &combine_context,
    );
    if let Some(early) = bootstrap_early_phase(&outcome) {
        return Ok(Err(BootstrapPhaseResult::from_snapshot(early)));
    }

    let BootstrapPlanOutcome::NeedsShape(bootstrap_plan) = outcome else {
        return Ok(Err(bootstrap_skipped("bootstrap_precheck_failed")));
    };
    let output_count = bootstrap_plan.output_amounts_base_units.len();
    let (fee_mojos, fee_source, fee_lookup_error) = resolve_bootstrap_split_fee(
        signer_config,
        program.coin_ops_minimum_fee_mojos,
        output_count,
    )
    .await;
    if fee_mojos > 0 {
        return Ok(Err(bootstrap_failed(BootstrapPhaseFailure::new(
            "signer_mixed_split_fee_not_supported",
            fee_mojos,
            fee_source,
            fee_lookup_error,
        ))));
    }

    let existing_coin_ids: HashSet<String> = asset_scoped_coins
        .iter()
        .map(|coin| coin.id.clone())
        .collect();

    Ok(Ok(BootstrapShapeContext {
        split_asset_id,
        split_asset_mojo_multiplier,
        receive_address: receive_address.to_string(),
        bootstrap_plan,
        ladder_entries,
        fee_mojos,
        fee_source,
        fee_lookup_error,
        existing_coin_ids,
        #[cfg(test)]
        test_overrides: test_overrides::SignerDenominationTestOverrides::default(),
    }))
}

#[must_use]
pub fn run_signer_denomination_phase<'a>(
    program: &'a ManagerProgramConfig,
    market: &'a MarketConfig,
    signer_config: &'a SignerConfig,
    resolved_base_asset_id: &'a str,
    resolved_quote_asset_id: &'a str,
    quote_price: f64,
    action_side: &'a str,
) -> SignerDenominationPhaseFuture<'a> {
    Box::pin(run_signer_denomination_phase_async(
        program,
        market,
        signer_config,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        quote_price,
        action_side,
    ))
}

// Clippy `large_futures`: the phase is already boxed at `run_signer_denomination_phase`.
#[allow(clippy::large_futures)]
async fn run_signer_denomination_phase_async(
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    signer_config: &SignerConfig,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    quote_price: f64,
    action_side: &str,
) -> SignerResult<BootstrapPhaseResult> {
    match prepare_bootstrap_execution_plan(
        program,
        signer_config,
        market,
        action_side,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        quote_price,
    )
    .await?
    {
        Ok(shape_ctx) => execute_bootstrap_shape(program, signer_config, shape_ctx).await,
        Err(result) => Ok(result),
    }
}

#[cfg(test)]
mod tests;
