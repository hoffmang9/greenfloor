use chia_protocol::{Bytes32, Coin};
use chia_sdk_coinset::CoinsetClient;
use chia_sdk_driver::Cat;
use serde::Serialize;

use crate::coinset::{
    client_for_network, list_unspent_cats, list_unspent_cats_by_ids, list_unspent_xch,
};
use crate::error::{SignerError, SignerResult};
use crate::vault::members::hex_to_bytes32;

#[derive(Debug, Clone, Serialize)]
pub struct CoinRecordSummary {
    pub coin_id: String,
    pub parent_coin_info: String,
    pub puzzle_hash: String,
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p2_puzzle_hash: Option<String>,
}

pub async fn list_xch_coin_summaries(
    network: &str,
    receive_address: &str,
) -> SignerResult<Vec<CoinRecordSummary>> {
    let client = client_for_network(network)?;
    let coins = list_unspent_xch(&client, receive_address).await?;
    Ok(coins.into_iter().map(coin_summary_from_coin).collect())
}

pub async fn list_cat_coin_summaries(
    network: &str,
    receive_address: &str,
    asset_id: &str,
) -> SignerResult<Vec<CoinRecordSummary>> {
    let client = client_for_network(network)?;
    let asset_bytes = hex_to_bytes32(asset_id)?;
    let cats = list_unspent_cats(&client, receive_address, asset_bytes).await?;
    Ok(cats.into_iter().map(cat_summary_from_cat).collect())
}

pub async fn list_cat_coin_summaries_by_ids(
    network: &str,
    coin_ids: &[String],
) -> SignerResult<Vec<CoinRecordSummary>> {
    let client = client_for_network(network)?;
    let parsed = crate::coinset::parse_coin_ids(coin_ids)?;
    let cats = list_unspent_cats_by_ids(&client, &parsed).await?;
    Ok(cats.into_iter().map(cat_summary_from_cat).collect())
}

fn coin_summary_from_coin(coin: Coin) -> CoinRecordSummary {
    let puzzle_hash = format!("0x{}", hex::encode(coin.puzzle_hash));
    CoinRecordSummary {
        coin_id: format!("0x{}", hex::encode(coin.coin_id())),
        parent_coin_info: format!("0x{}", hex::encode(coin.parent_coin_info)),
        puzzle_hash: puzzle_hash.clone(),
        amount: coin.amount,
        p2_puzzle_hash: Some(puzzle_hash),
    }
}

fn cat_summary_from_cat(cat: Cat) -> CoinRecordSummary {
    CoinRecordSummary {
        coin_id: format!("0x{}", hex::encode(cat.coin.coin_id())),
        parent_coin_info: format!("0x{}", hex::encode(cat.coin.parent_coin_info)),
        puzzle_hash: format!("0x{}", hex::encode(cat.coin.puzzle_hash)),
        amount: cat.coin.amount,
        p2_puzzle_hash: Some(format!("0x{}", hex::encode(cat.info.p2_puzzle_hash))),
    }
}

pub fn is_xch_like_asset(asset_id: &str) -> bool {
    matches!(
        asset_id.trim().to_lowercase().as_str(),
        "" | "xch" | "txch" | "1"
    )
}

pub fn cat_asset_bytes(asset_id: &str) -> SignerResult<Bytes32> {
    hex_to_bytes32(asset_id)
}
