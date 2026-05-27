//! Shared coin selection for BLS offer and mixed-split paths.

use std::collections::HashSet;

use chia_protocol::{Bytes32, Coin};
use chia_sdk_driver::Cat;
use chia_sdk_utils::select_coins;

use crate::coinset::{
    list_unspent_cats, list_unspent_cats_by_ids, list_unspent_xch, select_cats_smallest_first,
    CoinsetClient,
};
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

pub async fn select_cats_smallest_for_amount(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
    empty_list_err: SignerError,
    insufficient_err: SignerError,
) -> SignerResult<Vec<Cat>> {
    let cats = if explicit_coin_ids.is_empty() {
        list_unspent_cats(client, receive_address, asset_id).await?
    } else {
        list_unspent_cats_by_ids(client, explicit_coin_ids).await?
    };
    if cats.is_empty() {
        return Err(empty_list_err);
    }
    let selected = select_cats_smallest_first(cats, target_amount);
    if selected.is_empty() {
        return Err(insufficient_err);
    }
    Ok(selected)
}

pub async fn select_cats_explicit_sum(
    client: &CoinsetClient,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
    insufficient: SignerError,
) -> SignerResult<Vec<Cat>> {
    let offered_cats = list_unspent_cats_by_ids(client, explicit_coin_ids).await?;
    let offered_total: u64 = offered_cats.iter().map(|cat| cat.coin.amount).sum();
    if offered_total < target_amount {
        return Err(insufficient);
    }
    Ok(offered_cats)
}
