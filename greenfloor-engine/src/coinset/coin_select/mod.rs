//! Coin listing and selection (CAT; shared by vault and BLS paths).

use chia_protocol::Bytes32;
use chia_sdk_coinset::{CoinRecord, CoinsetClient};
use chia_sdk_driver::Cat;

use super::cats::{
    cats_with_lineage_from_records, coin_records_for_cat_outer_puzzle_hash,
    list_unspent_cats_by_ids,
};
use super::parse::unspent_coin_records;
use crate::error::{SignerError, SignerResult};

/// Minimum CAT output amount for offer/dust policy (1000 mojos = 1 CAT unit).
pub const MIN_CAT_OUTPUT_MOJOS: u64 = 1000;

#[derive(Debug, Clone)]
pub struct SelectedCats {
    pub selected: Vec<Cat>,
    pub offered_total: u64,
    pub change_amount: u64,
}

#[must_use]
fn select_smallest_first_by_amount<T: Copy>(
    items: Vec<T>,
    target_total: u64,
    amount: impl Fn(&T) -> u64,
) -> Vec<T> {
    if target_total == 0 {
        return Vec::new();
    }
    if let Some(item) = items
        .iter()
        .find(|item| amount(item) == target_total)
        .copied()
    {
        return vec![item];
    }
    if let Some(item) = items
        .iter()
        .filter(|item| amount(item) >= target_total)
        .min_by_key(|item| amount(item))
        .copied()
    {
        return vec![item];
    }
    let mut sorted = items;
    sorted.sort_by_key(|item| amount(item));
    let mut selected = Vec::new();
    let mut running = 0u64;
    for item in sorted {
        running = running.saturating_add(amount(&item));
        selected.push(item);
        if running >= target_total {
            return selected;
        }
    }
    Vec::new()
}

#[must_use]
pub fn select_cats_smallest_first(cats: Vec<Cat>, target_total: u64) -> Vec<Cat> {
    select_smallest_first_by_amount(cats, target_total, |cat| cat.coin.amount)
}

/// How to reduce a CAT list to the coins that cover *`target_amount`*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoinSelectionMode {
    /// Smallest-first subset until the running total reaches the target.
    SmallestFirst,
    /// Use every listed coin; fail when the sum is below the target.
    ExplicitSum,
}

impl CoinSelectionMode {
    /// Wallet listing uses smallest-first; explicit coin ids use the full set.
    pub fn from_explicit_ids(explicit_coin_ids: &[Bytes32]) -> Self {
        if explicit_coin_ids.is_empty() {
            CoinSelectionMode::SmallestFirst
        } else {
            CoinSelectionMode::ExplicitSum
        }
    }
}

fn select_from_list<T: Copy>(
    items: Vec<T>,
    target_amount: u64,
    mode: CoinSelectionMode,
    amount: impl Fn(&T) -> u64,
    empty_list_err: SignerError,
    insufficient_err: SignerError,
) -> SignerResult<Vec<T>> {
    if items.is_empty() {
        return Err(empty_list_err);
    }
    let selected = match mode {
        CoinSelectionMode::SmallestFirst => {
            select_smallest_first_by_amount(items, target_amount, &amount)
        }
        CoinSelectionMode::ExplicitSum => items,
    };
    if selected.is_empty() {
        return Err(insufficient_err);
    }
    let offered_total: u64 = selected.iter().map(&amount).sum();
    if offered_total < target_amount {
        return Err(insufficient_err);
    }
    Ok(selected)
}

fn finalize_amount_selection<T: Copy>(
    items: Vec<T>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
    amount: impl Fn(&T) -> u64,
) -> SignerResult<(Vec<T>, u64)> {
    let mode = CoinSelectionMode::from_explicit_ids(explicit_coin_ids);
    let selected = select_from_list(
        items,
        target_amount,
        mode,
        &amount,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )?;
    let offered_total: u64 = selected.iter().map(&amount).sum();
    Ok((selected, offered_total))
}

pub(crate) fn finalize_selected_cats(
    cats: Vec<Cat>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let (selected, offered_total) =
        finalize_amount_selection(cats, explicit_coin_ids, target_amount, |cat| {
            cat.coin.amount
        })?;
    Ok(SelectedCats {
        change_amount: offered_total.saturating_sub(target_amount),
        selected,
        offered_total,
    })
}

async fn finalize_selected_coin_records(
    client: &CoinsetClient,
    records: Vec<CoinRecord>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let (selected_records, offered_total) =
        finalize_amount_selection(records, explicit_coin_ids, target_amount, |record| {
            record.coin.amount
        })?;
    let selected = cats_with_lineage_from_records(client, &selected_records).await?;
    Ok(SelectedCats {
        change_amount: offered_total.saturating_sub(target_amount),
        selected,
        offered_total,
    })
}

pub(crate) async fn select_cats_for_spend(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    if explicit_coin_ids.is_empty() {
        let records = unspent_coin_records(
            coin_records_for_cat_outer_puzzle_hash(client, receive_address, asset_id).await?,
        )
        .collect();
        return finalize_selected_coin_records(client, records, explicit_coin_ids, target_amount)
            .await;
    }
    let cats = list_unspent_cats_by_ids(client, explicit_coin_ids).await?;
    finalize_selected_cats(cats, explicit_coin_ids, target_amount)
}

#[cfg(test)]
mod tests;
