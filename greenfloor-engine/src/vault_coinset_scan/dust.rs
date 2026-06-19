use crate::hex::normalize_hex_id;
use crate::vault_coinset_scan::types::{CoinKind, CoinRow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustCoin {
    pub coin_id: String,
    pub amount: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DustBatchPlan {
    pub combinable_batches: Vec<Vec<DustCoin>>,
    pub uncombinable: Vec<DustCoin>,
}

#[must_use]
pub fn dust_coins_from_scan(coins: &[CoinRow], dust_threshold_mojos: u64) -> Vec<DustCoin> {
    let mut out = Vec::new();
    for row in coins {
        if row.kind != CoinKind::Cat {
            continue;
        }
        if row.spent_block_index != 0 {
            continue;
        }
        if row.amount == 0 || row.amount >= dust_threshold_mojos {
            continue;
        }
        let coin_id = normalize_hex_id(&row.coin_id);
        if coin_id.is_empty() {
            continue;
        }
        out.push(DustCoin {
            coin_id,
            amount: row.amount,
        });
    }
    out
}

#[must_use]
pub fn plan_dust_batches(coins: &[DustCoin], batch_size: usize) -> DustBatchPlan {
    let size = batch_size.max(2);
    if coins.is_empty() {
        return DustBatchPlan {
            combinable_batches: Vec::new(),
            uncombinable: Vec::new(),
        };
    }
    let full_batches = coins.len() / size;
    let combinable_batches = coins
        .chunks(size)
        .take(full_batches)
        .map(<[DustCoin]>::to_vec)
        .collect();
    let uncombinable = coins[full_batches * size..].to_vec();
    DustBatchPlan {
        combinable_batches,
        uncombinable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coinset::finalize_selected_cats;
    use crate::coinset::test_support::cat_with_amount;
    use crate::vault::members::hex_to_bytes32;
    use crate::vault_coinset_scan::types::CoinRow;

    fn cat_row(coin_id: &str, amount: u64, spent: u64) -> CoinRow {
        CoinRow {
            coin_id: coin_id.to_string(),
            puzzle_hash: "b".repeat(64),
            parent_coin_info: "c".repeat(64),
            amount,
            confirmed_block_index: 1,
            spent_block_index: spent,
            discovered_nonces: vec![1],
            discovered_by_puzzle_hash: true,
            discovered_by_hint: false,
            kind: CoinKind::Cat,
            cat_asset_id: Some("d".repeat(64)),
            cat_symbols: vec![],
        }
    }

    #[test]
    fn dust_coins_from_scan_filters_spent_and_threshold() {
        let cat = "a".repeat(64);
        let coins = vec![
            cat_row(&cat, 500, 0),
            cat_row(&"b".repeat(64), 1000, 0),
            cat_row(&"c".repeat(64), 100, 1),
            cat_row(&"d".repeat(64), 1, 0),
        ];
        let got = dust_coins_from_scan(&coins, 1000);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].coin_id, cat);
        assert_eq!(got[0].amount, 500);
        assert_eq!(got[1].amount, 1);
    }

    #[test]
    fn plan_dust_batches_keeps_orphans_out_of_combinable_batches() {
        let coins: Vec<DustCoin> = (0..5)
            .map(|i| DustCoin {
                coin_id: format!("{i:064x}"),
                amount: 1,
            })
            .collect();
        let plan = plan_dust_batches(&coins, 2);
        assert_eq!(plan.combinable_batches.len(), 2);
        assert_eq!(plan.combinable_batches[0].len(), 2);
        assert_eq!(plan.combinable_batches[1].len(), 2);
        assert_eq!(plan.uncombinable.len(), 1);
        assert_eq!(
            plan.uncombinable[0].coin_id,
            "0000000000000000000000000000000000000000000000000000000000000004"
        );
    }

    #[test]
    fn scan_dust_batch_hydrates_for_mixed_split_selection() {
        let rows = vec![
            cat_row(&"a".repeat(64), 400, 0),
            cat_row(&"b".repeat(64), 300, 0),
            cat_row(&"c".repeat(64), 1000, 0),
        ];
        let dust = dust_coins_from_scan(&rows, 1000);
        let plan = plan_dust_batches(&dust, 2);
        assert_eq!(plan.combinable_batches.len(), 1);
        assert_eq!(plan.combinable_batches[0].len(), 2);
        assert!(plan.uncombinable.is_empty());

        let batch = &plan.combinable_batches[0];
        let coin_ids: Vec<_> = batch
            .iter()
            .map(|coin| hex_to_bytes32(&coin.coin_id).expect("coin id"))
            .collect();
        let target_total: u64 = batch.iter().map(|coin| coin.amount).sum();
        let hydrated = vec![cat_with_amount(400), cat_with_amount(300)];
        let selected = finalize_selected_cats(hydrated, &coin_ids, target_total)
            .expect("explicit dust batch should cover mixed-split target");
        assert_eq!(selected.selected.len(), 2);
        assert_eq!(selected.offered_total, target_total);
        assert_eq!(selected.change_amount, 0);
    }
}
