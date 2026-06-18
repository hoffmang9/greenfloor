//! Runtime signer denomination bootstrap (vault mixed-split) for offer build/post.
//!
//! Deterministic ladder planning lives in `offer::bootstrap`; this module executes
//! the signer-side denomination phase before offer construction.

mod planning;
mod split_submit;
mod types;
mod wait;

use std::collections::HashSet;

use crate::coinset::list_wallet_unspent_coins;
use crate::config::{ManagerProgramConfig, MarketConfig, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_early_phase, bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapCoin,
    BootstrapPlanOutcome,
};
use crate::offer::request::{normalize_offer_side, signer_split_asset_id};

pub use types::{bootstrap_blocks_offer, BootstrapPhaseResult};

use planning::{
    bootstrap_ladder_entries_for_side, resolve_bootstrap_split_fee, wallet_coin_spendable,
};
use split_submit::submit_bootstrap_mixed_split;
use types::BootstrapPhaseFailure;
use wait::wait_for_coinset_confirmation;

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
        return Ok(BootstrapPhaseResult::skipped(format!(
            "missing_{side}_ladder"
        )));
    }

    let ladder_entries = bootstrap_ladder_entries_for_side(
        side,
        &side_ladder,
        &market.pricing,
        quote_price,
        resolved_quote_asset_id,
    )?;
    if ladder_entries.is_empty() {
        return Ok(BootstrapPhaseResult::skipped(format!(
            "empty_{side}_ladder_after_quote_conversion"
        )));
    }

    let split_asset_id =
        signer_split_asset_id(side, resolved_base_asset_id, resolved_quote_asset_id);
    if split_asset_id.trim().is_empty() {
        return Ok(BootstrapPhaseResult::skipped(format!(
            "missing_{side}_asset_for_bootstrap"
        )));
    }

    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Ok(BootstrapPhaseResult::skipped(
            "missing_receive_address_for_bootstrap",
        ));
    }

    let asset_scoped_coins =
        match list_wallet_unspent_coins(&program.network, receive_address, &split_asset_id).await {
            Ok(coins) => coins,
            Err(err) => {
                return Ok(BootstrapPhaseResult::skipped(format!(
                    "bootstrap_coin_list_failed:{err}"
                )));
            }
        };

    let spendable_coins: Vec<BootstrapCoin> = asset_scoped_coins
        .iter()
        .filter(|coin| wallet_coin_spendable(coin))
        .map(|coin| BootstrapCoin {
            id: coin.id.clone(),
            amount: i64::try_from(coin.amount).unwrap_or(i64::MAX),
        })
        .collect();

    let outcome = plan_bootstrap_mixed_outputs(&ladder_entries, &spendable_coins);
    if let Some(early) = bootstrap_early_phase(&outcome) {
        return Ok(BootstrapPhaseResult::from_snapshot(early));
    }
    let BootstrapPlanOutcome::NeedsSplit(bootstrap_plan) = outcome else {
        return Ok(BootstrapPhaseResult::skipped("bootstrap_precheck_failed"));
    };

    let (fee_mojos, fee_source, fee_lookup_error) = resolve_bootstrap_split_fee(
        &program.network,
        program.coin_ops_minimum_fee_mojos,
        bootstrap_plan.output_amounts_base_units.len(),
    )
    .await;
    if fee_mojos > 0 {
        return Ok(BootstrapPhaseResult::failed(BootstrapPhaseFailure::new(
            "signer_mixed_split_fee_not_supported",
            fee_mojos,
            fee_source,
            fee_lookup_error,
        )));
    }

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
            return Ok(BootstrapPhaseResult::failed(
                BootstrapPhaseFailure::new(
                    format!("signer_mixed_split_error:{err}"),
                    fee_mojos,
                    fee_source,
                    fee_lookup_error,
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
            return Ok(BootstrapPhaseResult::failed(
                BootstrapPhaseFailure::new(
                    "bootstrap_wait_failed",
                    fee_mojos,
                    fee_source,
                    fee_lookup_error,
                )
                .with_plan(bootstrap_plan)
                .with_wait_error(err.to_string())
                .with_split_result(split_result),
            ));
        }
    };

    let refreshed_asset_coins =
        list_wallet_unspent_coins(&program.network, receive_address, &split_asset_id).await?;
    let refreshed_spendable: Vec<BootstrapCoin> = refreshed_asset_coins
        .iter()
        .filter(|coin| wallet_coin_spendable(coin))
        .map(|coin| BootstrapCoin {
            id: coin.id.clone(),
            amount: i64::try_from(coin.amount).unwrap_or(i64::MAX),
        })
        .collect();
    let remaining = plan_bootstrap_mixed_outputs(&ladder_entries, &refreshed_spendable);
    let executed = bootstrap_executed_phase(&remaining);
    Ok(BootstrapPhaseResult {
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
    })
}
