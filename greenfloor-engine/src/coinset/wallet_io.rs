use std::collections::HashSet;

use crate::bech32m::{decode_address, decode_offer};
use chia_protocol::SpendBundle;
use chia_puzzle_types::cat::CatArgs;
use chia_traits::Streamable;
use serde::Serialize;

use super::{
    cats, direct_api, direct_coinset_client, is_xch_like_asset, json_util::to_coinset_hex,
    xch::list_unspent_xch,
};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes32;
use crate::hex::normalize_hex_id;

#[derive(Debug, Clone, Serialize)]
pub struct WalletUnspentCoin {
    pub id: String,
    pub name: String,
    pub amount: u64,
    pub state: String,
}

fn wallet_coin_from_id(coin_id: impl AsRef<[u8]>, amount: u64) -> WalletUnspentCoin {
    let id = normalize_hex_id(&hex::encode(coin_id.as_ref()));
    WalletUnspentCoin {
        name: id.clone(),
        id,
        amount,
        state: "CONFIRMED".to_string(),
    }
}

/// Spend bundle hex.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn spend_bundle_hex(spend_bundle: &SpendBundle) -> SignerResult<String> {
    Ok(hex::encode(spend_bundle.to_bytes().map_err(|err| {
        SignerError::Other(format!("failed to serialize spend bundle: {err}"))
    })?))
}

/// List wallet unspent coins for a signer on the operator network.
///
/// Coinset host resolution uses `operator_network` plus `signer.coinset_base_url`, not
/// `signer.network`, so callers must pass the program/CLI network even before combine
/// context rewrites the signer block.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn list_wallet_unspent_coins_for_signer(
    operator_network: &str,
    signer: &SignerConfig,
    receive_address: &str,
    asset_id: &str,
) -> SignerResult<Vec<WalletUnspentCoin>> {
    let endpoint =
        direct_api::resolve_coinset_endpoint(operator_network, &signer.coinset_base_url, None);
    list_wallet_unspent_coins(
        endpoint.network,
        receive_address,
        asset_id,
        Some(endpoint.base_url.as_str()),
    )
    .await
}

/// List wallet unspent coins.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn list_wallet_unspent_coins(
    network: &str,
    receive_address: &str,
    asset_id: &str,
    coinset_base_url: Option<&str>,
) -> SignerResult<Vec<WalletUnspentCoin>> {
    let client = direct_coinset_client(network, coinset_base_url)?;
    if is_xch_like_asset(asset_id) {
        let coins = list_unspent_xch(&client, receive_address).await?;
        return Ok(coins
            .into_iter()
            .filter(|coin| coin.amount > 0)
            .map(|coin| wallet_coin_from_id(coin.coin_id(), coin.amount))
            .collect());
    }
    let asset_bytes = hex_to_bytes32(asset_id)?;
    let cats = cats::list_unspent_cats(&client, receive_address, asset_bytes).await?;
    Ok(cats
        .into_iter()
        .filter(|cat| cat.coin.amount > 0)
        .map(|cat| wallet_coin_from_id(cat.coin.coin_id(), cat.coin.amount))
        .collect())
}

/// Spend bundle hash from hex.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn spend_bundle_hash_from_hex(spend_bundle_hex: &str) -> SignerResult<String> {
    let raw = spend_bundle_hex
        .strip_prefix("0x")
        .unwrap_or(spend_bundle_hex);
    let bytes = hex::decode(raw)
        .map_err(|err| SignerError::Other(format!("invalid spend_bundle_hex: {err}")))?;
    let bundle = SpendBundle::from_bytes(&bytes)
        .map_err(|err| SignerError::Other(format!("invalid spend bundle: {err}")))?;
    Ok(to_coinset_hex(bundle.hash().as_ref()))
}

/// Puzzle hash hex for receive address.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn puzzle_hash_hex_for_receive_address(receive_address: &str) -> SignerResult<String> {
    let puzzle_hash = decode_address(receive_address)?;
    Ok(to_coinset_hex(puzzle_hash.as_ref()))
}

/// Cat outer puzzle hash hex.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn cat_outer_puzzle_hash_hex(receive_address: &str, asset_id: &str) -> SignerResult<String> {
    let puzzle_hash = decode_address(receive_address)?;
    let asset_bytes = hex_to_bytes32(asset_id)?;
    let cat_outer: [u8; 32] = CatArgs::curry_tree_hash(asset_bytes, puzzle_hash.into()).into();
    Ok(to_coinset_hex(&cat_outer))
}

/// Inventory puzzle hashes for one market receive address (inner + optional CAT outer).
///
/// `base_asset_id` should already be a resolved CAT asset id. Pass `None` (or xch/txch)
/// for XCH-only markets.
///
/// # Errors
///
/// Returns an error if the receive address is empty or cannot be decoded, or if a CAT
/// outer hash cannot be derived when a non-XCH base asset id is provided.
pub fn market_inventory_p2s(
    receive_address: &str,
    base_asset_id: Option<&str>,
) -> SignerResult<Vec<String>> {
    let receive = receive_address.trim();
    if receive.is_empty() {
        return Err(SignerError::Other(
            "receive_address is required for market inventory p2s".to_string(),
        ));
    }
    let mut p2s = Vec::new();
    let mut seen = HashSet::new();
    let inner = normalize_hex_id(&puzzle_hash_hex_for_receive_address(receive)?);
    if inner.len() != 64 {
        return Err(SignerError::Other(format!(
            "receive puzzle hash for `{receive}` is not 64 hex chars (len={})",
            inner.len()
        )));
    }
    seen.insert(inner.clone());
    p2s.push(inner);

    let Some(base) = base_asset_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(p2s);
    };
    if base.eq_ignore_ascii_case("xch") || base.eq_ignore_ascii_case("txch") {
        return Ok(p2s);
    }
    let outer = normalize_hex_id(&cat_outer_puzzle_hash_hex(receive, base)?);
    if outer.len() != 64 {
        return Err(SignerError::Other(format!(
            "CAT outer puzzle hash for `{receive}` / `{base}` is not 64 hex chars (len={})",
            outer.len()
        )));
    }
    if seen.insert(outer.clone()) {
        p2s.push(outer);
    }
    Ok(p2s)
}

/// Extract coin id hints from offer text.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn extract_coin_id_hints_from_offer_text(offer_text: &str) -> SignerResult<Vec<String>> {
    let spend_bundle = decode_offer(offer_text)?;
    let mut hints = Vec::new();
    let mut seen = HashSet::new();
    for coin_spend in &spend_bundle.coin_spends {
        let normalized = normalize_hex_id(&hex::encode(coin_spend.coin.coin_id()));
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        hints.push(normalized);
    }
    Ok(hints)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spend_bundle_hash_from_hex_rejects_garbage() {
        let err = spend_bundle_hash_from_hex("not-hex").expect_err("invalid hex");
        assert!(err.to_string().contains("invalid spend_bundle_hex"));
    }

    #[test]
    fn extract_coin_id_hints_from_offer_text_rejects_garbage() {
        let err = extract_coin_id_hints_from_offer_text("not-an-offer").expect_err("invalid offer");
        assert!(!err.to_string().is_empty());
    }
}
