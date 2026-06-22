use chia_protocol::Coin;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};

use super::cats::decode_receive_address;
use super::client_retry::with_client_retries;
use super::parse::{coin_records_from_response, unspent_coin_records};
use crate::error::SignerResult;

/// List unspent xch.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn list_unspent_xch(
    client: &CoinsetClient,
    receive_address: &str,
) -> SignerResult<Vec<Coin>> {
    let puzzle_hash = decode_receive_address(receive_address)?;
    let response = with_client_retries(|| async {
        client
            .get_coin_records_by_puzzle_hash(puzzle_hash, None, None, Some(false), None)
            .await
    })
    .await?;
    Ok(unspent_coin_records(coin_records_from_response(response)?)
        .map(|record| record.coin)
        .collect())
}
