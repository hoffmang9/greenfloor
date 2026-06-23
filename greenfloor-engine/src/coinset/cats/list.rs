use chia_protocol::{Bytes32, Coin};
use chia_puzzle_types::cat::CatArgs;
use chia_sdk_coinset::{ChiaRpcClient, CoinRecord, CoinsetClient};
use chia_sdk_driver::Cat;
use futures_util::future::try_join_all;

use super::{coin_records_from_response, resolve, unspent_coin_records};
use crate::bech32m::decode_address;
use crate::coinset::retry::with_coinset_client_retries;
use crate::error::SignerResult;

pub(crate) async fn coin_records_for_cat_outer_puzzle_hash(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
) -> SignerResult<Vec<CoinRecord>> {
    let p2_puzzle_hash = decode_address(receive_address)?;
    let cat_outer_puzzle_hash = CatArgs::curry_tree_hash(asset_id, p2_puzzle_hash.into()).into();
    let response = with_coinset_client_retries(|| async {
        client
            .get_coin_records_by_puzzle_hash(cat_outer_puzzle_hash, None, None, Some(false), None)
            .await
    })
    .await?;
    coin_records_from_response(response)
}

/// List unspent CAT coins for a known asset id and receive address (single puzzle-hash RPC).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn list_unspent_cat_coins(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
) -> SignerResult<Vec<Coin>> {
    Ok(unspent_coin_records(
        coin_records_for_cat_outer_puzzle_hash(client, receive_address, asset_id).await?,
    )
    .map(|record| record.coin)
    .filter(|coin| coin.amount > 0)
    .collect())
}

/// Resolve spendable [`Cat`] values with lineage proofs for coin records.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn cats_with_lineage_from_records(
    client: &CoinsetClient,
    records: &[CoinRecord],
) -> SignerResult<Vec<Cat>> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    let resolved = try_join_all(records.iter().copied().map(|record| {
        let client = client.clone();
        async move { resolve::cat_from_record(&client, &record).await }
    }))
    .await?;
    Ok(resolved.into_iter().flatten().collect())
}

/// Fetch coin records for the given coin ids (missing ids are omitted).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn coin_records_for_coin_ids(
    client: &CoinsetClient,
    coin_ids: &[Bytes32],
) -> SignerResult<Vec<CoinRecord>> {
    if coin_ids.is_empty() {
        return Ok(Vec::new());
    }
    let responses = try_join_all(coin_ids.iter().copied().map(|coin_id| {
        let client = client.clone();
        async move {
            with_coinset_client_retries(|| async { client.get_coin_record_by_name(coin_id).await })
                .await
        }
    }))
    .await?;
    Ok(responses
        .into_iter()
        .filter_map(|response| response.coin_record)
        .collect())
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
    let records: Vec<CoinRecord> = unspent_coin_records(
        coin_records_for_coin_ids(client, coin_ids).await?,
    )
    .collect();
    cats_with_lineage_from_records(client, &records).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

    #[tokio::test]
    async fn list_unspent_cat_coins_uses_puzzle_hash_query_only() {
        let body = r#"{
        "success": true,
        "coin_records": [{
            "coin": {
                "parent_coin_info": "c325057d788bee13367cb8e2d71ff3e209b5e94b31b296322ba1a143053fef5b",
                "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                "amount": 5000
            },
            "coinbase": false,
            "confirmed_block_index": 1,
            "spent": false,
            "spent_block_index": 0,
            "timestamp": 1
        }]
    }"#;
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;
        let client = CoinsetClient::new(server.url());
        let asset_id = Bytes32::new([0xae; 32]);
        let coins = list_unspent_cat_coins(&client, RECEIVE_ADDRESS, asset_id)
            .await
            .expect("coins");
        mock.assert_async().await;
        assert_eq!(coins.len(), 1);
        assert_eq!(coins[0].amount, 5000);
    }
}
