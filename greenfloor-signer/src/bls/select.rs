//! Shared XCH coin selection for BLS offer and mixed-split paths.

use std::collections::HashSet;

use chia_protocol::{Bytes32, Coin};
use chia_sdk_utils::select_coins;

use crate::coinset::{list_unspent_xch, CoinsetClient};
use crate::error::{SignerError, SignerResult};

pub async fn select_xch_for_amount(
    client: &CoinsetClient,
    receive_address: &str,
    explicit_coin_ids: &[Bytes32],
    amount: u64,
    empty_err: SignerError,
    select_failed: SignerError,
) -> SignerResult<Vec<Coin>> {
    let mut xch_coins = list_unspent_xch(client, receive_address).await?;
    if !explicit_coin_ids.is_empty() {
        let allowed: HashSet<Bytes32> = explicit_coin_ids.iter().copied().collect();
        xch_coins.retain(|coin| allowed.contains(&coin.coin_id()));
    }
    if xch_coins.is_empty() {
        return Err(empty_err);
    }
    select_coins(xch_coins, amount).map_err(|_| select_failed)
}
