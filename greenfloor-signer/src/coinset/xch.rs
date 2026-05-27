use chia_protocol::Coin;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};

use crate::coinset::decode_receive_address;
use crate::error::{SignerError, SignerResult};

pub async fn list_unspent_xch(
    client: &CoinsetClient,
    receive_address: &str,
) -> SignerResult<Vec<Coin>> {
    let puzzle_hash = decode_receive_address(receive_address)?;
    let response = client
        .get_coin_records_by_puzzle_hash(puzzle_hash, None, None, Some(false), None)
        .await
        .map_err(SignerError::from)?;
    if !response.success {
        return Err(SignerError::Coinset(
            response
                .error
                .unwrap_or_else(|| "coinset request failed".to_string()),
        ));
    }
    let records = response.coin_records.unwrap_or_default();
    let mut coins = Vec::new();
    for record in records {
        if record.spent {
            continue;
        }
        coins.push(record.coin);
    }
    Ok(coins)
}
