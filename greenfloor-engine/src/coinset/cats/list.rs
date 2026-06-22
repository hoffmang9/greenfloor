use chia_protocol::Bytes32;
use chia_puzzle_types::cat::CatArgs;
use chia_sdk_coinset::{ChiaRpcClient, CoinRecord, CoinsetClient};
use chia_sdk_driver::Cat;

use super::super::parse::{coin_records_from_response, unspent_coin_records};
use super::puzzle_hash::decode_receive_address;
use super::resolve;
use crate::error::SignerResult;

async fn unspent_cats_from_records(
    client: &CoinsetClient,
    records: Vec<CoinRecord>,
) -> SignerResult<Vec<Cat>> {
    let mut cats = Vec::new();
    for record in unspent_coin_records(records) {
        if let Some(cat) = resolve::cat_from_record(client, &record).await? {
            cats.push(cat);
        }
    }
    Ok(cats)
}

/// List unspent cats.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn list_unspent_cats(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
) -> SignerResult<Vec<Cat>> {
    let puzzle_hash = decode_receive_address(receive_address)?;
    let cat_outer_puzzle_hash = CatArgs::curry_tree_hash(asset_id, puzzle_hash.into()).into();
    let response = client
        .get_coin_records_by_puzzle_hash(cat_outer_puzzle_hash, None, None, Some(false), None)
        .await
        .map_err(crate::error::SignerError::from)?;
    unspent_cats_from_records(client, coin_records_from_response(response)?).await
}

/// List unspent cats by ids.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn list_unspent_cats_by_ids(
    client: &CoinsetClient,
    coin_ids: &[Bytes32],
) -> SignerResult<Vec<Cat>> {
    let mut records = Vec::with_capacity(coin_ids.len());
    for coin_id in coin_ids {
        let response = client
            .get_coin_record_by_name(*coin_id)
            .await
            .map_err(crate::error::SignerError::from)?;
        if let Some(record) = response.coin_record {
            records.push(record);
        }
    }
    unspent_cats_from_records(client, records).await
}
