use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::is_spendable_coin_state;
use crate::config::{load_markets_config_with_overlay, load_program_config, require_signer_offer_path};
use crate::error::{SignerError, SignerResult};

use super::context::{resolve_asset_filter, select_list_market};
use crate::manager_cli::json::emit_json;

pub async fn run_coins_list(
    program_path: &Path,
    markets_path: &Path,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    let _ = vault_id;
    if let Err(err) = require_signer_offer_path(program_path) {
        emit_json(&json!({
            "ok": false,
            "error": "coin_list_requires_signer_backend",
            "detail": err.to_string(),
        }))?;
        return Ok(2);
    }
    let program = load_program_config(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, None)?;
    let market = select_list_market(&markets)?;
    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Err(SignerError::Other(
            "market missing receive_address for signer coin list".to_string(),
        ));
    }
    let filter = cat_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| asset.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string));
    let filter_label = filter.clone();
    let list_asset_id = if let Some(filter_value) = filter {
        resolve_asset_filter(program_path, &program.network, &filter_value).await?
    } else {
        market.base_asset.clone()
    };
    let coins = crate::coinset::list_wallet_unspent_coins(
        &program.network,
        receive_address,
        &list_asset_id,
    )
    .await?;
    let min_amount = crate::coin_ops::coin_op_min_amount_mojos(market.base_asset.trim());
    let items: Vec<Value> = coins
        .iter()
        .map(|coin| {
            let state = coin.state.trim().to_ascii_uppercase();
            let spendable = is_spendable_coin_state(&state)
                && i64::try_from(coin.amount).unwrap_or(0) >= min_amount;
            json!({
                "coin_id": coin.name,
                "amount": coin.amount,
                "state": state,
                "pending": state == "PENDING" || state == "MEMPOOL",
                "spendable": spendable,
                "asset": list_asset_id,
                "reported_asset": filter_label,
                "scoped_asset": filter_label,
            })
        })
        .collect();
    let spendable_items: Vec<_> = items
        .iter()
        .filter(|row| row.get("spendable").and_then(Value::as_bool) == Some(true))
        .collect();
    let spendable_amount: u64 = spendable_items
        .iter()
        .filter_map(|row| row.get("amount").and_then(Value::as_u64))
        .sum();
    emit_json(&json!({
        "execution_backend": "signer",
        "network": program.network,
        "market_id": market.market_id,
        "receive_address": receive_address,
        "resolved_asset_id": filter_label,
        "asset": list_asset_id,
        "coin_count": items.len(),
        "spendable_coin_count": spendable_items.len(),
        "spendable_count": spendable_items.len(),
        "spendable_amount": spendable_amount,
        "coins": items,
    }))?;
    Ok(0)
}

pub async fn run_coin_status(
    program_path: &Path,
    markets_path: &Path,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    run_coins_list(program_path, markets_path, asset, vault_id, cat_id).await
}
