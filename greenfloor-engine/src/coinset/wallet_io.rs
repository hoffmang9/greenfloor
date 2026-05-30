use std::collections::HashSet;

use chia_protocol::SpendBundle;
use chia_puzzle_types::cat::CatArgs;
use chia_sdk_driver::decode_offer;
use chia_traits::Streamable;
use serde::Serialize;

use crate::coinset::{
    client_for_network, decode_receive_address, is_xch_like_asset, list_unspent_cats,
    list_unspent_xch,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::vault::members::hex_to_bytes32;

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

pub async fn list_wallet_unspent_coins(
    network: &str,
    receive_address: &str,
    asset_id: &str,
) -> SignerResult<Vec<WalletUnspentCoin>> {
    let client = client_for_network(network)?;
    if is_xch_like_asset(asset_id) {
        let coins = list_unspent_xch(&client, receive_address).await?;
        return Ok(coins
            .into_iter()
            .filter(|coin| coin.amount > 0)
            .map(|coin| wallet_coin_from_id(coin.coin_id(), coin.amount))
            .collect());
    }
    let asset_bytes = hex_to_bytes32(asset_id)?;
    let cats = list_unspent_cats(&client, receive_address, asset_bytes).await?;
    Ok(cats
        .into_iter()
        .filter(|cat| cat.coin.amount > 0)
        .map(|cat| wallet_coin_from_id(cat.coin.coin_id(), cat.coin.amount))
        .collect())
}

pub fn spend_bundle_hash_from_hex(spend_bundle_hex: &str) -> SignerResult<String> {
    let raw = spend_bundle_hex
        .strip_prefix("0x")
        .unwrap_or(spend_bundle_hex);
    let bytes = hex::decode(raw)
        .map_err(|err| SignerError::Other(format!("invalid spend_bundle_hex: {err}")))?;
    let bundle = SpendBundle::from_bytes(&bytes)
        .map_err(|err| SignerError::Other(format!("invalid spend bundle: {err}")))?;
    Ok(format!("0x{}", hex::encode(bundle.hash())))
}

pub fn puzzle_hash_hex_for_receive_address(receive_address: &str) -> SignerResult<String> {
    let puzzle_hash = decode_receive_address(receive_address)?;
    Ok(format!("0x{}", hex::encode(puzzle_hash)))
}

pub fn cat_outer_puzzle_hash_hex(receive_address: &str, asset_id: &str) -> SignerResult<String> {
    let puzzle_hash = decode_receive_address(receive_address)?;
    let asset_bytes = hex_to_bytes32(asset_id)?;
    let cat_outer: [u8; 32] = CatArgs::curry_tree_hash(asset_bytes, puzzle_hash.into()).into();
    Ok(format!("0x{}", hex::encode(cat_outer)))
}

pub fn extract_coin_id_hints_from_offer_text(offer_text: &str) -> SignerResult<Vec<String>> {
    let spend_bundle = decode_offer(offer_text).map_err(|err| SignerError::Driver(err.to_string()))?;
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
