use chia_protocol::Bytes32;
use chia_puzzle_types::cat::CatArgs;
use chia_sdk_coinset::{ChiaRpcClient, CoinRecord, CoinsetClient};
use chia_sdk_driver::{Cat, CatInfo};
use futures_util::future::try_join_all;

use super::{coin_records_from_response, decode_receive_address, resolve, unspent_coin_records};
use crate::coinset::client_retry::with_client_retries;
use crate::error::SignerResult;

fn cat_from_scoped_puzzle_hash_record(
    asset_id: Bytes32,
    p2_puzzle_hash: Bytes32,
    record: &CoinRecord,
) -> Cat {
    Cat::new(
        record.coin,
        None,
        CatInfo::new(asset_id, None, p2_puzzle_hash),
    )
}

fn cats_from_scoped_puzzle_hash_records(
    asset_id: Bytes32,
    p2_puzzle_hash: Bytes32,
    records: Vec<CoinRecord>,
) -> Vec<Cat> {
    unspent_coin_records(records)
        .map(|record| cat_from_scoped_puzzle_hash_record(asset_id, p2_puzzle_hash, &record))
        .collect()
}

async fn unspent_cats_from_records_with_lineage(
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

async fn coin_records_for_cat_outer_puzzle_hash(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
) -> SignerResult<(Bytes32, Vec<CoinRecord>)> {
    let p2_puzzle_hash = decode_receive_address(receive_address)?;
    let cat_outer_puzzle_hash = CatArgs::curry_tree_hash(asset_id, p2_puzzle_hash.into()).into();
    let response = with_client_retries(|| async {
        client
            .get_coin_records_by_puzzle_hash(cat_outer_puzzle_hash, None, None, Some(false), None)
            .await
    })
    .await?;
    Ok((p2_puzzle_hash, coin_records_from_response(response)?))
}

/// List unspent cats scoped to a known asset id and receive address.
///
/// Uses a single `get_coin_records_by_puzzle_hash` query against the CAT outer puzzle hash.
/// Lineage proofs are not fetched; callers that need spendable [`Cat`] metadata for vault
/// spends should use [`list_unspent_cats_with_lineage`].
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn list_unspent_cats(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
) -> SignerResult<Vec<Cat>> {
    let (p2_puzzle_hash, records) =
        coin_records_for_cat_outer_puzzle_hash(client, receive_address, asset_id).await?;
    Ok(cats_from_scoped_puzzle_hash_records(
        asset_id,
        p2_puzzle_hash,
        records,
    ))
}

/// List unspent cats and resolve lineage proofs from parent spends.
///
/// Used by coin selection for vault CAT spends where [`Cat::lineage_proof`] is required.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn list_unspent_cats_with_lineage(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
) -> SignerResult<Vec<Cat>> {
    let (_p2_puzzle_hash, records) =
        coin_records_for_cat_outer_puzzle_hash(client, receive_address, asset_id).await?;
    unspent_cats_from_records_with_lineage(client, records).await
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
    if coin_ids.is_empty() {
        return Ok(Vec::new());
    }
    let responses = try_join_all(coin_ids.iter().copied().map(|coin_id| {
        let client = client.clone();
        async move {
            with_client_retries(|| async { client.get_coin_record_by_name(coin_id).await }).await
        }
    }))
    .await?;
    let records = responses
        .into_iter()
        .filter_map(|response| response.coin_record)
        .collect();
    unspent_cats_from_records_with_lineage(client, records).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

    #[tokio::test]
    async fn list_unspent_cats_maps_puzzle_hash_records_without_parent_lookups() {
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
        let cats = list_unspent_cats(&client, RECEIVE_ADDRESS, asset_id)
            .await
            .expect("cats");
        mock.assert_async().await;
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0].coin.amount, 5000);
        assert_eq!(cats[0].info.asset_id, asset_id);
        assert!(cats[0].lineage_proof.is_none());
    }
}
