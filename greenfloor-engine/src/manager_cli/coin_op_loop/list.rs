use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::is_spendable_coin_state;
use crate::config::{
    load_markets_config_with_overlay, parse_program_config, program_bundle_from_parsed,
    read_program_yaml, ProgramConfigBundle,
};
use crate::error::{SignerError, SignerResult};

use crate::manager_cli::context::ManagerContext;

use super::context::{resolve_asset_filter, select_list_market};

struct CoinListSnapshot {
    network: String,
    market_id: String,
    receive_address: String,
    list_asset_id: String,
    filter_label: Option<String>,
    items: Vec<Value>,
    spendable_coin_count: usize,
    spendable_amount: u64,
    pending_coin_count: usize,
}

async fn load_coin_list_snapshot(
    bundle: &ProgramConfigBundle,
    markets_path: &Path,
    asset: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<CoinListSnapshot> {
    let program = &bundle.program;
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
        .or_else(|| {
            asset
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    let filter_label = filter.clone();
    let list_asset_id = if let Some(filter_value) = filter {
        resolve_asset_filter(&bundle.signer, &filter_value).await?
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
    let pending_coin_count = items
        .iter()
        .filter(|row| row.get("pending").and_then(Value::as_bool) == Some(true))
        .count();
    Ok(CoinListSnapshot {
        network: program.network.clone(),
        market_id: market.market_id.clone(),
        receive_address: receive_address.to_string(),
        list_asset_id,
        filter_label,
        spendable_coin_count: spendable_items.len(),
        spendable_amount,
        pending_coin_count,
        items,
    })
}

async fn run_coin_list_command(
    mgr: &ManagerContext,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
    op: &str,
) -> SignerResult<i32> {
    let _ = vault_id;
    let raw = read_program_yaml(&mgr.program_config)?;
    let program = parse_program_config(&raw)?;
    if let Err(err) = program.require_signer_offer_path() {
        mgr.emit_json(&json!({
            "ok": false,
            "error": "coin_list_requires_signer_backend",
            "detail": err.to_string(),
        }))?;
        return Ok(2);
    }
    let bundle = program_bundle_from_parsed(program, &raw)?;
    let snapshot = load_coin_list_snapshot(&bundle, &mgr.markets_config, asset, cat_id).await?;
    if op == "coin-status" {
        mgr.emit_json(&json!({
            "op": "coin-status",
            "network": snapshot.network,
            "market_id": snapshot.market_id,
            "receive_address": snapshot.receive_address,
            "resolved_asset_id": snapshot.filter_label,
            "asset": snapshot.list_asset_id,
            "total_coin_count": snapshot.items.len(),
            "spendable_coin_count": snapshot.spendable_coin_count,
            "spendable_amount": snapshot.spendable_amount,
            "pending_coin_count": snapshot.pending_coin_count,
        }))?;
    } else {
        mgr.emit_json(&json!({
            "network": snapshot.network,
            "market_id": snapshot.market_id,
            "receive_address": snapshot.receive_address,
            "resolved_asset_id": snapshot.filter_label,
            "asset": snapshot.list_asset_id,
            "coin_count": snapshot.items.len(),
            "spendable_coin_count": snapshot.spendable_coin_count,
            "spendable_amount": snapshot.spendable_amount,
            "coins": snapshot.items,
        }))?;
    }
    Ok(0)
}

pub async fn run_coins_list(
    mgr: &ManagerContext,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    run_coin_list_command(mgr, asset, vault_id, cat_id, "coins-list").await
}

pub async fn run_coin_status(
    mgr: &ManagerContext,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    run_coin_list_command(mgr, asset, vault_id, cat_id, "coin-status").await
}
