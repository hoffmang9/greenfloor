use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::execution::CoinOpExecContext;
#[cfg(test)]
use crate::coin_ops::execution::CoinOpTestOverrides;
use crate::coin_ops::SpendableCoin;
use crate::config::{load_gated_operator_market, MarketConfig};
use crate::error::{SignerError, SignerResult};
use crate::hex::{is_hex_id, normalize_hex_id};
use crate::offer::resolve_market_base_asset_id;

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
    let loaded = load_gated_operator_market(
        program_path,
        markets_path,
        testnet_markets_path,
        network,
        market_id,
        pair,
    )?;
    CoinOpExecContext::new(
        loaded.program,
        loaded.signer,
        loaded.market,
        asset_id_override,
        HashSet::default(),
        #[cfg(test)]
        CoinOpTestOverrides::default(),
    )
    .await
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
    resolve_market_base_asset_id(signer_config, filter).await
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
