//! Runtime signer denomination bootstrap (vault mixed-split) for offer build/post.
//!
//! Deterministic ladder planning lives in `offer::bootstrap`; this module executes
//! the signer-side denomination phase before offer construction.

mod planning;
mod split_submit;
mod types;
mod wait;

use std::collections::HashSet;

use crate::coinset::{list_wallet_unspent_coins, WalletUnspentCoin};
use crate::config::{ManagerProgramConfig, MarketConfig, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_early_phase, bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapCoin,
    BootstrapPlan, BootstrapPlanOutcome, PlannerLadderRow,
};
use crate::offer::request::{normalize_offer_side, signer_split_asset_id};

pub use types::{bootstrap_blocks_offer, BootstrapPhaseResult};

use planning::{
    bootstrap_ladder_entries_for_side, resolve_bootstrap_split_fee, wallet_coin_spendable,
};
use split_submit::submit_bootstrap_mixed_split;
use types::BootstrapPhaseFailure;
use wait::wait_for_coinset_confirmation;

fn bootstrap_skipped(reason: impl Into<String>) -> BootstrapPhaseResult {
    BootstrapPhaseResult::skipped(reason)
}

fn bootstrap_failed(failure: BootstrapPhaseFailure) -> BootstrapPhaseResult {
    BootstrapPhaseResult::failed(failure)
}

fn spendable_bootstrap_coins(coins: &[WalletUnspentCoin]) -> Vec<BootstrapCoin> {
    coins
        .iter()
        .filter(|coin| wallet_coin_spendable(coin))
        .map(|coin| BootstrapCoin {
            id: coin.id.clone(),
            amount: i64::try_from(coin.amount).unwrap_or(i64::MAX),
        })
        .collect()
}

async fn load_asset_scoped_coins(
    program: &ManagerProgramConfig,
    receive_address: &str,
    split_asset_id: &str,
) -> Result<Vec<WalletUnspentCoin>, BootstrapPhaseResult> {
    list_wallet_unspent_coins(&program.network, receive_address, split_asset_id)
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

struct ExecutedAfterSplitParams<'a> {
    fee_mojos: u64,
    fee_source: String,
    fee_lookup_error: Option<String>,
    split_result: serde_json::Value,
    wait_events: Vec<serde_json::Value>,
    bootstrap_plan: BootstrapPlan,
    ladder_entries: &'a [PlannerLadderRow],
    refreshed_spendable: &'a [BootstrapCoin],
}

fn executed_after_split(params: ExecutedAfterSplitParams<'_>) -> BootstrapPhaseResult {
    let ExecutedAfterSplitParams {
        fee_mojos,
        fee_source,
        fee_lookup_error,
        split_result,
        wait_events,
        bootstrap_plan,
        ladder_entries,
        refreshed_spendable,
    } = params;
    let remaining = plan_bootstrap_mixed_outputs(ladder_entries, refreshed_spendable);
    let executed = bootstrap_executed_phase(&remaining);
    BootstrapPhaseResult {
        status: executed.status.to_string(),
        reason: executed.reason,
        ready: executed.ready,
        fee_mojos,
        fee_source,
        fee_lookup_error,
        wait_error: None,
        split_result,
        wait_events,
        plan: Some(bootstrap_plan),
    }
}

pub async fn run_signer_denomination_phase(
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    signer_config: &SignerConfig,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    quote_price: f64,
    action_side: &str,
) -> SignerResult<BootstrapPhaseResult> {
    let side = normalize_offer_side(action_side);
    let side_ladder = market.ladders.get(side).cloned().unwrap_or_default();
    if side_ladder.is_empty() {
        return Ok(bootstrap_skipped(format!("missing_{side}_ladder")));
    }

    let ladder_entries = bootstrap_ladder_entries_for_side(
        side,
        &side_ladder,
        &market.pricing,
        quote_price,
        resolved_quote_asset_id,
    )?;
    if ladder_entries.is_empty() {
        return Ok(bootstrap_skipped(format!(
            "empty_{side}_ladder_after_quote_conversion"
        )));
    }

    let split_asset_id =
        signer_split_asset_id(side, resolved_base_asset_id, resolved_quote_asset_id);
    if split_asset_id.trim().is_empty() {
        return Ok(bootstrap_skipped(format!(
            "missing_{side}_asset_for_bootstrap"
        )));
    }

    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Ok(bootstrap_skipped("missing_receive_address_for_bootstrap"));
    }

    let asset_scoped_coins =
        match load_asset_scoped_coins(program, receive_address, &split_asset_id).await {
            Ok(coins) => coins,
            Err(result) => return Ok(result),
        };

    let spendable_coins = spendable_bootstrap_coins(&asset_scoped_coins);
    let outcome = plan_bootstrap_mixed_outputs(&ladder_entries, &spendable_coins);
    if let Some(early) = bootstrap_early_phase(&outcome) {
        return Ok(BootstrapPhaseResult::from_snapshot(early));
    }
    let BootstrapPlanOutcome::NeedsSplit(bootstrap_plan) = outcome else {
        return Ok(bootstrap_skipped("bootstrap_precheck_failed"));
    };

    let (fee_mojos, fee_source, fee_lookup_error) = resolve_bootstrap_split_fee(
        &program.network,
        program.coin_ops_minimum_fee_mojos,
        bootstrap_plan.output_amounts_base_units.len(),
    )
    .await;
    if fee_mojos > 0 {
        return Ok(bootstrap_failed(BootstrapPhaseFailure::new(
            "signer_mixed_split_fee_not_supported",
            fee_mojos,
            fee_source,
            fee_lookup_error,
        )));
    }

    let fee_context = (fee_mojos, fee_source.clone(), fee_lookup_error.clone());
    let existing_coin_ids: HashSet<String> = asset_scoped_coins
        .iter()
        .map(|coin| coin.id.clone())
        .collect();

    let split_result = match submit_bootstrap_mixed_split(
        signer_config,
        &bootstrap_plan,
        &split_asset_id,
        receive_address,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok(bootstrap_failed(
                BootstrapPhaseFailure::new(
                    format!("signer_mixed_split_error:{err}"),
                    fee_context.0,
                    fee_context.1,
                    fee_context.2,
                )
                .with_plan(bootstrap_plan),
            ));
        }
    };

    let wait_events = match wait_for_coinset_confirmation(
        &program.network,
        receive_address,
        &split_asset_id,
        &existing_coin_ids,
        program.runtime_offer_bootstrap_wait_timeout_seconds,
    )
    .await
    {
        Ok(events) => events,
        Err(err) => {
            return Ok(bootstrap_failed(
                BootstrapPhaseFailure::new(
                    "bootstrap_wait_failed",
                    fee_context.0,
                    fee_context.1,
                    fee_context.2,
                )
                .with_plan(bootstrap_plan)
                .with_wait_error(err.to_string())
                .with_split_result(split_result),
            ));
        }
    };

    let refreshed_asset_coins =
        list_wallet_unspent_coins(&program.network, receive_address, &split_asset_id).await?;
    let refreshed_spendable = spendable_bootstrap_coins(&refreshed_asset_coins);
    Ok(executed_after_split(ExecutedAfterSplitParams {
        fee_mojos: fee_context.0,
        fee_source: fee_context.1,
        fee_lookup_error: fee_context.2,
        split_result,
        wait_events,
        bootstrap_plan,
        ladder_entries: &ladder_entries,
        refreshed_spendable: &refreshed_spendable,
    }))
}
