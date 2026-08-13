//! Vault-controlled CAT balance: receive inventory plus known unreturned makers.

use std::collections::HashSet;

use crate::coinset::OfferCoinsetBackend;
use crate::cycle::{unreturned_row_priority, ReconcileState};
use crate::error::{OfferError, SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, normalize_hex_id};
use crate::storage::SqliteStore;

use super::amounts::vault_controlled_total;

#[must_use]
fn cat_matches_asset_filter(cat_asset_id: &str, filter_asset_id: &str) -> bool {
    normalize_hex_id(cat_asset_id) == normalize_hex_id(filter_asset_id)
}

/// One unreturned maker coin included in vault-controlled balance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreturnedMakerCoin {
    pub coin_id: String,
    pub amount: u64,
    pub fixed_delegated_puzzle_hash: String,
    pub offer_id: String,
    pub state: String,
    pub size_base_units: Option<i64>,
    pub reclaimable: bool,
}

/// Receive inventory plus known unreturned makers for one CAT asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultControlledBalance {
    pub receive_amount: u64,
    pub unreturned_amount: u64,
    pub vault_controlled_amount: u64,
    pub unreturned_coins: Vec<UnreturnedMakerCoin>,
}

/// Query unreturned makers, fetch CAT amounts, and sum vault-controlled inventory.
///
/// # Errors
///
/// Returns an error when `SQLite` or Coinset lookups fail.
pub async fn vault_controlled_balance<C: OfferCoinsetBackend>(
    store: &SqliteStore,
    backend: &C,
    market_id: &str,
    filter_asset_id: &str,
    receive_amount: u64,
) -> SignerResult<VaultControlledBalance> {
    let mut makers: Vec<_> = store
        .list_unreturned_presplit_makers(Some(market_id))?
        .into_iter()
        .map(|row| {
            let state = ReconcileState::parse(&row.state).ok();
            (row, state)
        })
        .collect();
    makers.sort_by(|(a, a_state), (b, b_state)| {
        unreturned_row_priority(a_state.as_ref())
            .cmp(&unreturned_row_priority(b_state.as_ref()))
            .then_with(|| a.offer_id.cmp(&b.offer_id))
    });

    let mut seen_coins = HashSet::new();
    let mut unreturned_amount = 0u64;
    let mut unreturned_coins = Vec::new();
    for (row, state) in makers {
        let coin_id = normalize_hex_id(&row.cancel_input_coin_id);
        if !seen_coins.insert(coin_id.clone()) {
            continue;
        }
        let Ok(bytes) = hex_to_bytes32(&coin_id) else {
            continue;
        };
        let amount = match backend.fetch_offer_input_cat(bytes).await {
            Ok(cat) => {
                let maker_asset = hex::encode(cat.info.asset_id);
                if !cat_matches_asset_filter(&maker_asset, filter_asset_id) {
                    continue;
                }
                cat.coin.amount
            }
            Err(SignerError::Offer(OfferError::PresplitCoinNotFound)) => continue,
            Err(err) => return Err(err),
        };
        unreturned_amount = unreturned_amount.saturating_add(amount);
        unreturned_coins.push(UnreturnedMakerCoin {
            coin_id,
            amount,
            fixed_delegated_puzzle_hash: normalize_hex_id(&row.fixed_delegated_puzzle_hash),
            offer_id: row.offer_id,
            state: row.state,
            size_base_units: row.size_base_units,
            reclaimable: state
                .as_ref()
                .is_some_and(ReconcileState::is_ops_reclaimable),
        });
    }

    Ok(VaultControlledBalance {
        receive_amount,
        unreturned_amount,
        vault_controlled_amount: vault_controlled_total(receive_amount, unreturned_amount),
        unreturned_coins,
    })
}

#[cfg(test)]
mod tests {
    use super::cat_matches_asset_filter;

    #[test]
    fn unreturned_makers_filter_by_asset_id() {
        let asset_a = "aa".repeat(32);
        let asset_b = "bb".repeat(32);
        assert!(cat_matches_asset_filter(&asset_a, &asset_a));
        assert!(cat_matches_asset_filter(&format!("0x{asset_a}"), &asset_a));
        assert!(!cat_matches_asset_filter(&asset_a, &asset_b));
    }
}
