//! `coins-balance`: vault-controlled CAT total (receive + known unreturned makers).

use std::collections::HashSet;

use serde_json::json;

use crate::coinset::{
    client_for_signer_on_network, list_wallet_unspent_coins_for_signer, LiveCoinset,
    OfferCoinsetBackend,
};
use crate::config::{
    load_gated_operator_market, GatedOperatorMarketLoadRequest, OperatorMarketCommand,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex_id};
use crate::manager_cli::context::ManagerContext;
use crate::storage::{resolve_state_db_path, SqliteStore};

/// Allowlisted idle/terminal states eligible for ops reclaim (open listings are never reclaimable).
fn state_is_reclaimable(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "expired" | "cancelled" | "cancel_submitted"
    )
}

#[must_use]
fn vault_controlled_total(receive_amount: u64, unreturned_amount: u64) -> u64 {
    receive_amount.saturating_add(unreturned_amount)
}

/// Prefer open/active rows when multiple `offer_state` rows share a maker coin id.
fn unreturned_row_priority(state: &str) -> u8 {
    match state.trim().to_ascii_lowercase().as_str() {
        "open" | "refresh_due" | "pending_visibility" | "mempool_observed" => 0,
        "expired" => 1,
        _ => 2,
    }
}

/// Vault-controlled balance for one asset: receive inventory + known unreturned makers.
///
/// # Errors
///
/// Returns an error when market load, Coinset, or `SQLite` queries fail.
pub async fn run_coins_balance(
    mgr: &ManagerContext,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    asset: Option<&str>,
) -> SignerResult<i32> {
    let loaded = load_gated_operator_market(&GatedOperatorMarketLoadRequest {
        program_path: &mgr.program_config,
        markets_path: &mgr.markets_config,
        testnet_markets_path: mgr.testnet_markets_path(),
        cats_path: Some(&mgr.cats_config),
        network,
        market_id,
        pair,
        command: OperatorMarketCommand::CoinList,
    })?;
    let market = &loaded.market_row;
    let receive_address = market.receive_address.trim();
    if receive_address.is_empty() {
        return Err(SignerError::Other(
            "market missing receive_address for coins-balance".to_string(),
        ));
    }
    let resolver = loaded.asset_resolver();
    let list_asset_id = if let Some(filter) = asset.map(str::trim).filter(|v| !v.is_empty()) {
        resolver.resolve_inventory_asset(filter).await?
    } else {
        market.base_asset.clone()
    };

    let receive_coins = list_wallet_unspent_coins_for_signer(
        &loaded.operator_network,
        &loaded.signer,
        receive_address,
        &list_asset_id,
    )
    .await?;
    let receive_amount: u64 = receive_coins.iter().map(|coin| coin.amount).sum();

    let db_path = resolve_state_db_path(&loaded.program.home_dir, mgr.state_db_override());
    let store = SqliteStore::open(&db_path)?;
    let mut makers = store.list_unreturned_presplit_makers(Some(&market.market_id))?;
    makers.sort_by(|a, b| {
        unreturned_row_priority(&a.state)
            .cmp(&unreturned_row_priority(&b.state))
            .then_with(|| a.offer_id.cmp(&b.offer_id))
    });

    let coinset = client_for_signer_on_network(&loaded.signer, &loaded.operator_network)?;
    let backend = LiveCoinset(&coinset);

    let mut seen_coins = HashSet::new();
    let mut unreturned_amount = 0u64;
    let mut unreturned_coins = Vec::new();
    for row in makers {
        let coin_id = normalize_hex_id(&row.cancel_input_coin_id);
        if !seen_coins.insert(coin_id.clone()) {
            continue;
        }
        let Ok(bytes) = hex_to_bytes32(&coin_id) else {
            continue;
        };
        let amount = match backend.fetch_offer_input_cat(bytes).await {
            Ok(cat) => cat.coin.amount,
            Err(SignerError::PresplitCoinNotFound) => continue,
            Err(err) => return Err(err),
        };
        unreturned_amount = unreturned_amount.saturating_add(amount);
        unreturned_coins.push(json!({
            "coin_id": coin_id,
            "amount": amount,
            "fixed_delegated_puzzle_hash": normalize_hex_id(&row.fixed_delegated_puzzle_hash),
            "offer_id": row.offer_id,
            "state": row.state,
            "size_base_units": row.size_base_units,
            "reclaimable": state_is_reclaimable(&row.state),
        }));
    }

    let vault_controlled_amount = vault_controlled_total(receive_amount, unreturned_amount);
    // CAT display units: 1000 mojos = 1 unit.
    let units = |mojos: u64| mojos / 1000;
    let payload = json!({
        "op": "coins-balance",
        "network": loaded.operator_network,
        "market_id": market.market_id,
        "asset": list_asset_id,
        "receive_address": receive_address,
        "receive_amount": receive_amount,
        "receive_units": units(receive_amount),
        "unreturned_amount": unreturned_amount,
        "unreturned_units": units(unreturned_amount),
        "vault_controlled_amount": vault_controlled_amount,
        "vault_controlled_units": units(vault_controlled_amount),
        "unreturned_coins": unreturned_coins,
        "note": "Open makers are listed with reclaimable=false; reclaim idle/expired via offers-reclaim-presplit.",
    });
    mgr.emit_json(&payload)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{state_is_reclaimable, vault_controlled_total};

    #[test]
    fn reclaimable_uses_idle_state_allowlist() {
        assert!(!state_is_reclaimable("open"));
        assert!(!state_is_reclaimable("refresh_due"));
        assert!(state_is_reclaimable("expired"));
        assert!(state_is_reclaimable("cancelled"));
        assert!(state_is_reclaimable("cancel_submitted"));
    }

    #[test]
    fn vault_controlled_sums_receive_and_unreturned() {
        assert_eq!(vault_controlled_total(1_000, 2_000), 3_000);
        assert_eq!(vault_controlled_total(u64::MAX, 1), u64::MAX);
    }
}
