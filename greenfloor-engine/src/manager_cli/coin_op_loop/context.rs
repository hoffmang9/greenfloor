use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::execution::{
    resolve_combine_input_cap, CoinOpExecContext, CoinOpTestOverrides,
};
use crate::coin_ops::SpendableCoin;
use crate::config::{
    load_markets_config_with_overlay, load_program_bundle_gated, resolve_market_for_build,
    MarketConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::{default_mojo_multiplier_for_asset, is_hex_id, normalize_hex_id};
use crate::offer::resolve_offer_assets_for_action;

pub(super) const COIN_SPLIT_LOCKUP_ERROR: &str =
    "coin_split_lockup_guardrail_would_lock_all_spendable_coins";
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
    let bundle = load_program_bundle_gated(program_path)?;
    let program = bundle.program;
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    let market = resolve_market_for_build(&markets, market_id, pair, network)?;
    let signer_config = bundle.signer;
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
        combine_input_cap: resolve_combine_input_cap(),
        watched_coin_ids: HashSet::default(),
        test_overrides: CoinOpTestOverrides::default(),
    })
}

pub(super) fn enforce_split_lockup_guardrail(
    spendable: &[SpendableCoin],
    selected_coin_ids: &[String],
    allow_lock_all_spendable: bool,
    resolved_asset_id: &str,
) -> Option<(i32, Value)> {
    if allow_lock_all_spendable {
        return None;
    }
    let spendable_ids: HashSet<_> = spendable.iter().map(|coin| coin.id.clone()).collect();
    let selected_set: HashSet<String> = selected_coin_ids.iter().cloned().collect();
    if spendable_ids.is_empty() || selected_set != spendable_ids {
        return None;
    }
    Some((
        2,
        json!({
            "error": COIN_SPLIT_LOCKUP_ERROR,
            "resolved_asset_id": resolved_asset_id,
            "spendable_asset_coin_count": spendable_ids.len(),
            "selected_spendable_coin_count": selected_set.len(),
        }),
    ))
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

pub(super) async fn resolve_asset_filter(
    signer_config: &crate::config::SignerConfig,
    filter: &str,
) -> SignerResult<String> {
    if is_hex_id(filter) {
        return Ok(normalize_hex_id(filter));
    }
    let (resolved, _) = resolve_offer_assets_for_action(signer_config, filter, "xch").await?;
    Ok(resolved)
}

pub(super) fn select_list_market(
    markets: &crate::config::MarketsConfig,
) -> SignerResult<&MarketConfig> {
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
