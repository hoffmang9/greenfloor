use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::is_spendable_coin_state;
use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::{load_gated_operator_market, OperatorMarketCommand};
use crate::error::{SignerError, SignerResult};

use crate::manager_cli::context::ManagerContext;

use super::context::resolve_asset_filter;

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

struct CoinListLoadParams<'a> {
    program_path: &'a Path,
    markets_path: &'a Path,
    testnet_markets_path: Option<&'a Path>,
    cats_path: &'a Path,
    network: &'a str,
    market_id: Option<&'a str>,
    pair: Option<&'a str>,
    asset: Option<&'a str>,
    cat_id: Option<&'a str>,
}

struct CoinListCommand<'a> {
    mgr: &'a ManagerContext,
    network: &'a str,
    market_id: Option<&'a str>,
    pair: Option<&'a str>,
    asset: Option<&'a str>,
    vault_id: Option<&'a str>,
    cat_id: Option<&'a str>,
    op: &'a str,
}

async fn load_coin_list_snapshot(params: CoinListLoadParams<'_>) -> SignerResult<CoinListSnapshot> {
    let CoinListLoadParams {
        program_path,
        markets_path,
        testnet_markets_path,
        cats_path,
        network,
        market_id,
        pair,
        asset,
        cat_id,
    } = params;
    let loaded = load_gated_operator_market(
        program_path,
        markets_path,
        testnet_markets_path,
        Some(cats_path),
        network,
        market_id,
        pair,
        OperatorMarketCommand::CoinList,
    )?;
    let market = loaded.market.clone();
    let resolver = loaded.asset_resolver();
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
        resolve_asset_filter(&resolver, &filter_value).await?
    } else {
        market.base_asset.clone()
    };
    let coins = list_wallet_unspent_coins_for_signer(
        &loaded.operator_network,
        &loaded.signer,
        receive_address,
        &list_asset_id,
    )
    .await?;
    let min_amount = crate::coin_ops::coin_op_min_amount_mojos(market.base_asset.trim());
    let items: Vec<Value> = coins
        .iter()
        .map(|coin| {
            let state = coin.state.trim().to_ascii_uppercase();
            let amount_i64 = i64::try_from(coin.amount).ok();
            let spendable = is_spendable_coin_state(&state)
                && amount_i64.is_some_and(|amount| amount >= min_amount);
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
        network: loaded.operator_network,
        market_id: market.market_id,
        receive_address: receive_address.to_string(),
        list_asset_id,
        filter_label,
        spendable_coin_count: spendable_items.len(),
        spendable_amount,
        pending_coin_count,
        items,
    })
}

async fn run_coin_list_command(cmd: CoinListCommand<'_>) -> SignerResult<i32> {
    let CoinListCommand {
        mgr,
        network,
        market_id,
        pair,
        asset,
        vault_id,
        cat_id,
        op,
    } = cmd;
    let _ = vault_id;
    let snapshot = match load_coin_list_snapshot(CoinListLoadParams {
        program_path: &mgr.program_config,
        markets_path: &mgr.markets_config,
        testnet_markets_path: mgr.testnet_markets_path(),
        cats_path: &mgr.cats_config,
        network,
        market_id,
        pair,
        asset,
        cat_id,
    })
    .await
    {
        Err(SignerError::SignerPathNotConfigured) => {
            mgr.emit_json(&json!({
                "ok": false,
                "error": "coin_list_requires_signer_backend",
                "detail": SignerError::SignerPathNotConfigured.to_string(),
            }))?;
            return Ok(2);
        }
        Err(err) => return Err(err),
        Ok(snapshot) => snapshot,
    };
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
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    run_coin_list_command(CoinListCommand {
        mgr,
        network,
        market_id,
        pair,
        asset,
        vault_id,
        cat_id,
        op: "coins-list",
    })
    .await
}

pub async fn run_coin_status(
    mgr: &ManagerContext,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    asset: Option<&str>,
    vault_id: Option<&str>,
    cat_id: Option<&str>,
) -> SignerResult<i32> {
    run_coin_list_command(CoinListCommand {
        mgr,
        network,
        market_id,
        pair,
        asset,
        vault_id,
        cat_id,
        op: "coin-status",
    })
    .await
}
