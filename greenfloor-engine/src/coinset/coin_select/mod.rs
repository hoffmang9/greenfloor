//! Coin listing and selection (CAT; shared by vault and BLS paths).

use std::collections::{HashMap, HashSet};

use chia_protocol::Bytes32;
use chia_sdk_coinset::{CoinRecord, CoinsetClient};
use chia_sdk_driver::Cat;

use super::cats::{
    cat_from_record, coin_records_for_cat_outer_puzzle_hash, coin_records_for_coin_ids,
};
use crate::coin_ops::{select_funding_coin_ids, FundingSelectionMode, SpendableCoin};
use crate::error::{SignerError, SignerResult};
use crate::hex::{bytes32_to_hex, normalize_hex_id};

/// Minimum CAT output amount for offer/dust policy (1000 mojos = 1 CAT unit).
pub const MIN_CAT_OUTPUT_MOJOS: u64 = 1000;

#[derive(Debug, Clone)]
pub struct SelectedCats {
    pub selected: Vec<Cat>,
    pub offered_total: u64,
    pub change_amount: u64,
}

fn amount_i64(amount: u64) -> i64 {
    i64::try_from(amount).unwrap_or(i64::MAX)
}

fn coin_spendable_id(coin_id: Bytes32) -> String {
    normalize_hex_id(&bytes32_to_hex(coin_id))
}

fn cat_to_spendable(cat: &Cat) -> SpendableCoin {
    SpendableCoin::with_puzzle_hash(
        coin_spendable_id(cat.coin.coin_id()),
        amount_i64(cat.coin.amount),
        normalize_hex_id(&bytes32_to_hex(cat.coin.puzzle_hash)),
    )
}

fn record_to_spendable(record: &CoinRecord) -> SpendableCoin {
    SpendableCoin::with_puzzle_hash(
        coin_spendable_id(record.coin.coin_id()),
        amount_i64(record.coin.amount),
        normalize_hex_id(&bytes32_to_hex(record.coin.puzzle_hash)),
    )
}

fn funding_mode_for_explicit_ids(explicit_coin_ids: &[Bytes32]) -> FundingSelectionMode {
    if explicit_coin_ids.is_empty() {
        FundingSelectionMode::SmallestFirst
    } else {
        FundingSelectionMode::AllListed
    }
}

fn reorder_by_ids<T>(items: Vec<T>, ids: &[String], id_of: impl Fn(&T) -> String) -> Vec<T> {
    let mut by_id: HashMap<String, T> = HashMap::with_capacity(items.len());
    for item in items {
        by_id.insert(id_of(&item), item);
    }
    ids.iter().filter_map(|id| by_id.remove(id)).collect()
}

fn select_from_spendable<T>(
    items: Vec<T>,
    target_amount: u64,
    mode: FundingSelectionMode,
    to_spendable: impl Fn(&T) -> SpendableCoin,
    amount: impl Fn(&T) -> u64,
) -> SignerResult<Vec<T>> {
    if items.is_empty() {
        return Err(SignerError::NoUnspentCatCoins);
    }
    let spendable: Vec<SpendableCoin> = items.iter().map(&to_spendable).collect();
    let ids = select_funding_coin_ids(mode, &spendable, amount_i64(target_amount), None, None);
    if ids.is_empty() {
        return Err(SignerError::InsufficientCatCoins);
    }
    let selected = reorder_by_ids(items, &ids, |item| to_spendable(item).id);
    let offered_total: u64 = selected.iter().map(&amount).sum();
    if offered_total < target_amount {
        return Err(SignerError::InsufficientCatCoins);
    }
    Ok(selected)
}

#[must_use]
pub fn select_cats_smallest_first(cats: Vec<Cat>, target_total: u64) -> Vec<Cat> {
    select_from_spendable(
        cats,
        target_total,
        FundingSelectionMode::SmallestFirst,
        cat_to_spendable,
        |cat| cat.coin.amount,
    )
    .unwrap_or_default()
}

fn finalize_amount_selection<T>(
    items: Vec<T>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
    to_spendable: impl Fn(&T) -> SpendableCoin,
    amount: impl Fn(&T) -> u64,
) -> SignerResult<(Vec<T>, u64)> {
    let selected = select_from_spendable(
        items,
        target_amount,
        funding_mode_for_explicit_ids(explicit_coin_ids),
        to_spendable,
        &amount,
    )?;
    let offered_total: u64 = selected.iter().map(amount).sum();
    Ok((selected, offered_total))
}

pub(crate) fn finalize_selected_cats(
    cats: Vec<Cat>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let (selected, offered_total) = finalize_amount_selection(
        cats,
        explicit_coin_ids,
        target_amount,
        cat_to_spendable,
        |cat| cat.coin.amount,
    )?;
    Ok(SelectedCats {
        change_amount: offered_total.saturating_sub(target_amount),
        selected,
        offered_total,
    })
}

pub(crate) fn finalize_preselected_cats_for_spend(
    cats: Vec<Cat>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    validate_preselected_cats_match_coin_ids(&cats, explicit_coin_ids)?;
    finalize_selected_cats(cats, explicit_coin_ids, target_amount)
}

fn validate_preselected_cats_match_coin_ids(
    cats: &[Cat],
    explicit_coin_ids: &[Bytes32],
) -> SignerResult<()> {
    if explicit_coin_ids.is_empty() {
        return Ok(());
    }
    if cats.len() != explicit_coin_ids.len() {
        return Err(SignerError::PreselectedCatCoinIdsMismatch);
    }
    let cat_ids: HashSet<Bytes32> = cats.iter().map(|cat| cat.coin.coin_id()).collect();
    if cat_ids.len() != cats.len() {
        return Err(SignerError::PreselectedCatCoinIdsMismatch);
    }
    for id in explicit_coin_ids {
        if !cat_ids.contains(id) {
            return Err(SignerError::PreselectedCatCoinIdsMismatch);
        }
    }
    Ok(())
}

pub(crate) async fn select_cats_for_spend(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let records = if explicit_coin_ids.is_empty() {
        coin_records_for_cat_outer_puzzle_hash(client, receive_address, asset_id)
            .await?
            .into_iter()
            .filter(|record| !record.spent)
            .collect()
    } else {
        coin_records_for_coin_ids(client, explicit_coin_ids)
            .await?
            .into_iter()
            .filter(|record| !record.spent)
            .collect()
    };
    select_cats_for_spend_from_records(client, records, explicit_coin_ids, target_amount).await
}

async fn select_cats_for_spend_from_records(
    client: &CoinsetClient,
    records: Vec<CoinRecord>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let mut excluded = HashSet::new();
    loop {
        let available: Vec<CoinRecord> = records
            .iter()
            .copied()
            .filter(|record| !excluded.contains(&record.coin.coin_id()))
            .collect();
        let (selected_records, _offered_total) = finalize_amount_selection(
            available,
            explicit_coin_ids,
            target_amount,
            record_to_spendable,
            |record| record.coin.amount,
        )?;
        let mut selected = Vec::with_capacity(selected_records.len());
        let mut unresolvable = Vec::new();
        for record in selected_records {
            match cat_from_record(client, &record).await? {
                Some(cat) => selected.push(cat),
                None => unresolvable.push(record.coin.coin_id()),
            }
        }
        if unresolvable.is_empty() {
            return finalize_selected_cats(selected, explicit_coin_ids, target_amount);
        }
        excluded.extend(unresolvable);
        if excluded.len() >= records.len() {
            return Err(SignerError::InsufficientCatCoins);
        }
    }
}

#[cfg(test)]
mod tests;
