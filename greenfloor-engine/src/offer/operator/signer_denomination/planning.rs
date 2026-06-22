use serde_json::{json, Value};

use crate::coin_ops::is_spendable_wallet_coin;
use crate::coinset::{get_conservative_fee_estimate, msp_base_url_for_signer, WalletUnspentCoin};
use crate::config::{LadderEntry, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::PlannerLadderRow;
use crate::offer::build_context::mojo_multiplier_for_leg;
use crate::offer::pricing::quote_mojos_for_base_size;
use crate::offer::request::normalize_offer_side;

pub(super) fn bootstrap_ladder_entries_for_side(
    side: &str,
    side_ladder: &[LadderEntry],
    pricing: &Value,
    quote_price: f64,
    resolved_quote_asset_id: &str,
) -> SignerResult<Vec<PlannerLadderRow>> {
    let side = normalize_offer_side(side);
    let mut quote_unit_multiplier: Option<i64> = None;
    if side == "buy" {
        quote_unit_multiplier = Some(mojo_multiplier_for_leg(
            pricing,
            "quote_unit_mojo_multiplier",
            resolved_quote_asset_id,
        ));
    }
    let mut entries = Vec::new();
    for entry in side_ladder {
        let mut size_base_units = entry.size_base_units;
        if let Some(multiplier) = quote_unit_multiplier {
            size_base_units = quote_mojos_for_base_size(size_base_units, quote_price, multiplier)?;
            if size_base_units <= 0 {
                continue;
            }
        }
        entries.push(PlannerLadderRow {
            size_base_units,
            target_count: entry.target_count,
            split_buffer_count: entry.split_buffer_count,
        });
    }
    Ok(entries)
}

fn bootstrap_fee_cost_for_output_count(output_count: usize) -> u64 {
    let count = u64::try_from(output_count.max(1)).unwrap_or(u64::MAX);
    1_000_000 + count.saturating_sub(1) * 250_000
}

pub(super) async fn resolve_bootstrap_split_fee(
    network: &str,
    signer: &SignerConfig,
    minimum_fee_mojos: u64,
    output_count: usize,
) -> (u64, String, Option<String>) {
    let fee_cost = bootstrap_fee_cost_for_output_count(output_count);
    let spend_count = u64::try_from(output_count.max(1)).unwrap_or(u64::MAX);
    match get_conservative_fee_estimate(
        network,
        msp_base_url_for_signer(signer),
        fee_cost,
        Some(spend_count),
    )
    .await
    {
        Ok(Some(fee_mojos)) => (fee_mojos, "coinset_conservative_fee".to_string(), None),
        Ok(None) => (
            minimum_fee_mojos,
            "config_minimum_fee_fallback".to_string(),
            None,
        ),
        Err(err) => (
            minimum_fee_mojos,
            "config_minimum_fee_fallback".to_string(),
            Some(err.to_string()),
        ),
    }
}

pub(super) fn wallet_coin_spendable(coin: &WalletUnspentCoin) -> bool {
    is_spendable_wallet_coin(&json!({
        "state": coin.state,
    }))
}

#[cfg(test)]
#[path = "planning_tests.rs"]
mod tests;
