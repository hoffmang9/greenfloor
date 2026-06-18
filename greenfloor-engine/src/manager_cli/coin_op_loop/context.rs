use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::{CoinCombineGateResult, CoinSplitGateResult, SpendableCoin, SplitCombinePrereqPlan};
use crate::coin_ops::execution::{combine_input_coin_cap, submit_combine_prereq, CoinOpExecContext};
use crate::config::{
    load_markets_config_with_overlay, load_program_config, load_signer_config,
    resolve_market_for_build, MarketConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex_id};
use crate::offer::resolve_offer_assets_for_action;

use crate::manager_cli::json::emit_json;

pub(super) const COIN_SPLIT_LOCKUP_ERROR: &str = "coin_split_lockup_guardrail_would_lock_all_spendable_coins";
pub(super) const COIN_SPLIT_NO_SPENDABLE_ERROR: &str = "no_spendable_split_coin_available";

pub(super) async fn build_coin_op_exec_context(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    asset_id_override: Option<&str>,
) -> SignerResult<CoinOpExecContext> {
    let program = load_program_config(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    let market = resolve_market_for_build(&markets, market_id, pair, network)?;
    let signer_config = load_signer_config(program_path)?;
    let canonical = asset_id_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(market.base_asset.trim());
    let (resolved_base_asset_id, _) =
        resolve_offer_assets_for_action(&signer_config, canonical, "xch").await?;
    Ok(CoinOpExecContext {
        signer_config,
        market: market.clone(),
        program: program.clone(),
        resolved_base_asset_id,
        base_unit_mojo_multiplier: default_mojo_multiplier_for_asset(market.base_asset.trim())
            as i64,
        combine_input_cap: combine_input_coin_cap(),
        watched_coin_ids: HashSet::new(),
    })
}

pub(super) async fn submit_split_combine_prereq(
    ctx: &CoinOpExecContext,
    prereq: &SplitCombinePrereqPlan,
) -> SignerResult<String> {
    submit_combine_prereq(ctx, &prereq.input_coin_ids).await
}

pub(super) fn enforce_split_lockup_guardrail(
    spendable: &[SpendableCoin],
    selected_coin_ids: &[String],
    allow_lock_all_spendable: bool,
    resolved_asset_id: &str,
) -> SignerResult<Option<i32>> {
    if allow_lock_all_spendable {
        return Ok(None);
    }
    let spendable_ids: HashSet<_> = spendable.iter().map(|coin| coin.id.clone()).collect();
    let selected_set: HashSet<String> = selected_coin_ids.iter().cloned().collect();
    if spendable_ids.is_empty() || selected_set != spendable_ids {
        return Ok(None);
    }
    emit_json(&json!({
        "error": COIN_SPLIT_LOCKUP_ERROR,
        "resolved_asset_id": resolved_asset_id,
        "spendable_asset_coin_count": spendable_ids.len(),
        "selected_spendable_coin_count": selected_set.len(),
    }))?;
    Ok(Some(2))
}

pub(super) fn spendable_coins_for_gate(spendable: &[SpendableCoin]) -> Vec<Value> {
    spendable
        .iter()
        .map(|coin| {
            json!({
                "amount": coin.amount,
                "state": "CONFIRMED",
            })
        })
        .collect()
}

pub(super) fn gate_to_json(gate: &CoinSplitGateResult) -> Value {
    json!({
        "asset_id": gate.asset_id,
        "size_base_units": gate.size_base_units,
        "required_min_count": gate.required_min_count,
        "current_count": gate.current_count,
        "larger_reserve_coin_count": gate.larger_reserve_coin_count,
        "extra_denom_coin_count": gate.extra_denom_coin_count,
        "reserve_ready": gate.reserve_ready,
        "ready": gate.ready,
    })
}

pub(super) fn combine_gate_to_json(gate: &CoinCombineGateResult) -> Value {
    json!({
        "asset_id": gate.asset_id,
        "size_base_units": gate.size_base_units,
        "max_allowed_count": gate.max_allowed_count,
        "current_count": gate.current_count,
        "ready": gate.ready,
    })
}

pub(super) async fn resolve_asset_filter(
    program_path: &Path,
    network: &str,
    filter: &str,
) -> SignerResult<String> {
    if is_hex_id(filter) {
        return Ok(normalize_hex_id(filter));
    }
    let signer_config = load_signer_config(program_path)?;
    let (resolved, _) = resolve_offer_assets_for_action(&signer_config, filter, "xch").await?;
    let _ = network;
    Ok(resolved)
}

pub(super) fn select_list_market(markets: &crate::config::MarketsConfig) -> SignerResult<&MarketConfig> {
    let enabled: Vec<_> = markets.markets.iter().filter(|m| m.enabled).collect();
    let candidates = if enabled.is_empty() {
        markets.markets.iter().collect::<Vec<_>>()
    } else {
        enabled
    };
    if candidates.is_empty() {
        return Err(SignerError::Other("no markets configured".to_string()));
    }
    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }
    Ok(candidates
        .iter()
        .min_by_key(|market| market.market_id.as_str())
        .copied()
        .expect("non-empty"))
}
