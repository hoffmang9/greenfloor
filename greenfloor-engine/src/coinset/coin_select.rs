//! Coin listing and selection (CAT; shared by vault and BLS paths).

use chia_protocol::Bytes32;
use chia_sdk_coinset::CoinsetClient;
use chia_sdk_driver::Cat;

use super::cats::{list_unspent_cats, list_unspent_cats_by_ids};
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
pub fn select_cats_smallest_first(cats: Vec<Cat>, target_total: u64) -> Vec<Cat> {
    let mut sorted = cats;
    sorted.sort_by_key(|cat| cat.coin.amount);
    let mut selected = Vec::new();
    let mut running = 0u64;
    for cat in sorted {
        running = running.saturating_add(cat.coin.amount);
        selected.push(cat);
        if running >= target_total {
            return selected;
        }
    }
    Vec::new()
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

/// Select CAT inputs from an already-listed coin set.
pub(crate) fn select_cats_from_list(
    cats: Vec<Cat>,
    target_amount: u64,
    mode: CoinSelectionMode,
    empty_list_err: SignerError,
    insufficient_err: SignerError,
) -> SignerResult<Vec<Cat>> {
    if cats.is_empty() {
        return Err(empty_list_err);
    }
    let selected = match mode {
        CoinSelectionMode::SmallestFirst => select_cats_smallest_first(cats, target_amount),
        CoinSelectionMode::ExplicitSum => cats,
    };
    if selected.is_empty() {
        return Err(insufficient_err);
    }
    let offered_total: u64 = selected.iter().map(|cat| cat.coin.amount).sum();
    if offered_total < target_amount {
        return Err(insufficient_err);
    }
    Ok(selected)
}

pub(crate) fn finalize_selected_cats(
    cats: Vec<Cat>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let mode = CoinSelectionMode::from_explicit_ids(explicit_coin_ids);
    let selected = select_cats_from_list(
        cats,
        target_amount,
        mode,
        SignerError::NoUnspentCatCoins,
        SignerError::InsufficientCatCoins,
    )?;
    let offered_total: u64 = selected.iter().map(|cat| cat.coin.amount).sum();
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
    let cats = if explicit_coin_ids.is_empty() {
        list_unspent_cats(client, receive_address, asset_id).await?
    } else {
        list_unspent_cats_by_ids(client, explicit_coin_ids).await?
    };
    finalize_selected_cats(cats, explicit_coin_ids, target_amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coinset::test_support::cat_with_amount;

    #[test]
    fn smallest_first_accumulates_until_target() {
        let cats = vec![
            cat_with_amount(5000),
            cat_with_amount(1000),
            cat_with_amount(3000),
        ];
        let selected = select_cats_from_list(
            cats,
            2500,
            CoinSelectionMode::SmallestFirst,
            SignerError::NoUnspentCatCoins,
            SignerError::InsufficientCatCoins,
        )
        .expect("selection");
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].coin.amount, 1000);
        assert_eq!(selected[1].coin.amount, 3000);
    }

    #[test]
    fn smallest_first_empty_list_uses_empty_error() {
        let err = select_cats_from_list(
            vec![],
            1000,
            CoinSelectionMode::SmallestFirst,
            SignerError::NoUnspentCatCoins,
            SignerError::InsufficientCatCoins,
        )
        .expect_err("empty");
        assert!(matches!(err, SignerError::NoUnspentCatCoins));
    }

    #[test]
    fn smallest_first_insufficient_uses_insufficient_error() {
        let err = select_cats_from_list(
            vec![cat_with_amount(500)],
            1000,
            CoinSelectionMode::SmallestFirst,
            SignerError::NoUnspentCatCoins,
            SignerError::InsufficientCatCoins,
        )
        .expect_err("insufficient");
        assert!(matches!(err, SignerError::InsufficientCatCoins));
    }

    #[test]
    fn explicit_sum_requires_full_set_total() {
        let selected = select_cats_from_list(
            vec![cat_with_amount(700), cat_with_amount(400)],
            1000,
            CoinSelectionMode::ExplicitSum,
            SignerError::NoUnspentCatCoins,
            SignerError::InsufficientCatCoins,
        )
        .expect("sum covers target");
        assert_eq!(selected.len(), 2);
        assert_eq!(
            selected.iter().map(|cat| cat.coin.amount).sum::<u64>(),
            1100
        );
    }

    #[test]
    fn explicit_sum_fails_when_total_below_target() {
        let err = select_cats_from_list(
            vec![cat_with_amount(400)],
            1000,
            CoinSelectionMode::ExplicitSum,
            SignerError::NoUnspentCatCoins,
            SignerError::InsufficientCatCoins,
        )
        .expect_err("below target");
        assert!(matches!(err, SignerError::InsufficientCatCoins));
    }

    #[test]
    fn finalize_selected_cats_uses_explicit_sum_for_fixed_ids() {
        let cats = vec![cat_with_amount(600), cat_with_amount(500)];
        let selected = finalize_selected_cats(cats, &[Bytes32::new([0xab; 32])], 1000)
            .expect("vault-style explicit selection");
        assert_eq!(selected.selected.len(), 2);
        assert_eq!(selected.offered_total, 1100);
        assert_eq!(selected.change_amount, 100);
    }
}
