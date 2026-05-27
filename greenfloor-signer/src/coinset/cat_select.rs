//! CAT coin listing and selection (shared by vault and BLS paths).

use chia_protocol::Bytes32;
use chia_sdk_driver::Cat;

use super::{
    list_unspent_cats, list_unspent_cats_by_ids, select_cats_smallest_first, CoinsetClient,
    SelectedCats,
};
use crate::error::{SignerError, SignerResult};

/// How to reduce a CAT list to the coins that cover *target_amount*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatSelectionMode {
    /// Smallest-first subset until the running total reaches the target.
    SmallestFirst,
    /// Use every listed coin; fail when the sum is below the target.
    ExplicitSum,
}

/// Select CAT inputs from an already-listed coin set.
pub(crate) fn select_cats_from_list(
    cats: Vec<Cat>,
    target_amount: u64,
    mode: CatSelectionMode,
    empty_list_err: SignerError,
    insufficient_err: SignerError,
) -> SignerResult<Vec<Cat>> {
    if cats.is_empty() {
        return Err(empty_list_err);
    }
    let selected = match mode {
        CatSelectionMode::SmallestFirst => select_cats_smallest_first(cats, target_amount),
        CatSelectionMode::ExplicitSum => cats,
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

/// List CAT coins (by wallet asset or explicit coin ids) and select per *mode*.
pub(crate) async fn list_and_select_cats(
    client: &CoinsetClient,
    receive_address: &str,
    asset_id: Bytes32,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
    mode: CatSelectionMode,
    empty_list_err: SignerError,
    insufficient_err: SignerError,
) -> SignerResult<Vec<Cat>> {
    let cats = if explicit_coin_ids.is_empty() {
        list_unspent_cats(client, receive_address, asset_id).await?
    } else {
        list_unspent_cats_by_ids(client, explicit_coin_ids).await?
    };
    select_cats_from_list(
        cats,
        target_amount,
        mode,
        empty_list_err,
        insufficient_err,
    )
}

pub(crate) fn finalize_selected_cats(
    cats: Vec<Cat>,
    explicit_coin_ids: &[Bytes32],
    target_amount: u64,
) -> SignerResult<SelectedCats> {
    let mode = if explicit_coin_ids.is_empty() {
        CatSelectionMode::SmallestFirst
    } else {
        CatSelectionMode::ExplicitSum
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::{Bytes32, Coin};
    use chia_sdk_driver::{Cat, CatInfo};

    fn cat_with_amount(amount: u64) -> Cat {
        Cat::new(
            Coin::new(Bytes32::new([amount as u8; 32]), Bytes32::default(), amount),
            None,
            CatInfo::new(Bytes32::new([0x01; 32]), None, Bytes32::default()),
        )
    }

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
            CatSelectionMode::SmallestFirst,
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
            CatSelectionMode::SmallestFirst,
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
            CatSelectionMode::SmallestFirst,
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
            CatSelectionMode::ExplicitSum,
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
            CatSelectionMode::ExplicitSum,
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
