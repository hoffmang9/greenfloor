//! Coin listing and selection (CAT; shared by vault and BLS paths).

use std::collections::{HashMap, HashSet};

use chia_protocol::{Bytes32, Coin};
use chia_sdk_coinset::{CoinRecord, CoinsetClient};
use chia_sdk_driver::Cat;

use super::cats::{
    cat_from_record, coin_records_for_cat_outer_puzzle_hash, coin_records_for_coin_ids,
};
use crate::coin_ops::{select_funding_coin_ids, FundingSelectionMode, SpendableCoin};
use crate::error::{CoinOpsError, SignerError, SignerResult};
use crate::hex::{bytes32_to_hex, normalize_hex_id};

/// Minimum CAT output amount for offer/dust policy (1000 mojos = 1 CAT unit).
pub const MIN_CAT_OUTPUT_MOJOS: u64 = 1000;

#[derive(Debug, Clone)]
pub struct SelectedCats {
    pub selected: Vec<Cat>,
    pub offered_total: u64,
    pub change_amount: u64,
}

impl SelectedCats {
    fn from_cats(selected: Vec<Cat>, target_amount: u64) -> Self {
        let offered_total: u64 = selected.iter().map(|cat| cat.coin.amount).sum();
        Self {
            change_amount: offered_total.saturating_sub(target_amount),
            selected,
            offered_total,
        }
    }
}

fn amount_i64(amount: u64) -> i64 {
    i64::try_from(amount).unwrap_or(i64::MAX)
}

fn coin_spendable_id(coin_id: Bytes32) -> String {
    normalize_hex_id(&bytes32_to_hex(coin_id))
}

fn coin_to_spendable(coin: &Coin) -> SpendableCoin {
    SpendableCoin::with_puzzle_hash(
        coin_spendable_id(coin.coin_id()),
        amount_i64(coin.amount),
        normalize_hex_id(&bytes32_to_hex(coin.puzzle_hash)),
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

fn select_items<T>(
    items: Vec<T>,
    target_amount: u64,
    mode: FundingSelectionMode,
    coin_of: impl Fn(&T) -> &Coin,
) -> SignerResult<Vec<T>> {
    if items.is_empty() {
        return Err(SignerError::CoinOps(CoinOpsError::NoUnspentCatCoins));
    }
    let spendable: Vec<SpendableCoin> = items
        .iter()
        .map(|item| coin_to_spendable(coin_of(item)))
        .collect();
    let ids = select_funding_coin_ids(mode, &spendable, amount_i64(target_amount), None, None);
    if ids.is_empty() {
        return Err(SignerError::CoinOps(CoinOpsError::InsufficientCatCoins));
    }
    let selected = reorder_by_ids(items, &ids, |item| {
        coin_spendable_id(coin_of(item).coin_id())
    });
    let offered_total: u64 = selected.iter().map(|item| coin_of(item).amount).sum();
    if offered_total < target_amount {
        return Err(SignerError::CoinOps(CoinOpsError::InsufficientCatCoins));
    }
    Ok(selected)
}

#[cfg(test)]
pub(crate) fn select_resolved_cats(
    cats: Vec<Cat>,
    target_amount: u64,
    mode: FundingSelectionMode,
) -> SignerResult<SelectedCats> {
    let selected = select_items(cats, target_amount, mode, |cat| &cat.coin)?;
    Ok(SelectedCats::from_cats(selected, target_amount))
}

pub(crate) fn finalize_preselected_cats_for_spend(
    cats: Vec<Cat>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    validate_preselected_cats_match_coin_ids(&cats, explicit_coin_ids)?;
    let selected = SelectedCats::from_cats(cats, target_amount);
    if selected.offered_total < target_amount {
        return Err(SignerError::CoinOps(CoinOpsError::InsufficientCatCoins));
    }
    Ok(selected)
}

fn validate_preselected_cats_match_coin_ids(
    cats: &[Cat],
    explicit_coin_ids: &[Bytes32],
) -> SignerResult<()> {
    if explicit_coin_ids.is_empty() {
        return Ok(());
    }
    if cats.len() != explicit_coin_ids.len() {
        return Err(SignerError::CoinOps(
            CoinOpsError::PreselectedCatCoinIdsMismatch,
        ));
    }
    let cat_ids: HashSet<Bytes32> = cats.iter().map(|cat| cat.coin.coin_id()).collect();
    if cat_ids.len() != cats.len() {
        return Err(SignerError::CoinOps(
            CoinOpsError::PreselectedCatCoinIdsMismatch,
        ));
    }
    for id in explicit_coin_ids {
        if !cat_ids.contains(id) {
            return Err(SignerError::CoinOps(
                CoinOpsError::PreselectedCatCoinIdsMismatch,
            ));
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
        let selected_records = select_items(
            available,
            target_amount,
            funding_mode_for_explicit_ids(explicit_coin_ids),
            |record| &record.coin,
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
            return Ok(SelectedCats::from_cats(selected, target_amount));
        }
        excluded.extend(unresolvable);
        if excluded.len() >= records.len() {
            return Err(SignerError::CoinOps(CoinOpsError::InsufficientCatCoins));
        }
    }
}

#[cfg(test)]
mod tests;
