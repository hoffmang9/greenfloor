use chia_protocol::Coin;
use chia_sdk_coinset::CoinsetClient;

use super::pagination::coin_records_by_puzzle_hash;
use super::parse::unspent_coin_records;
use crate::bech32m::decode_address;
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
    let puzzle_hash = decode_address(receive_address)?;
    let records = coin_records_by_puzzle_hash(client, puzzle_hash, None, None, Some(false)).await?;
    Ok(unspent_coin_records(records)
        .map(|record| record.coin)
        .collect())
}
