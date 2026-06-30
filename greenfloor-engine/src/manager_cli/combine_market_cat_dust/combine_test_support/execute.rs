use crate::hex::hex_to_bytes32;
use crate::vault::mixed_split::MixedSplitResult;
use crate::vault_coinset_scan::{
    DustBatchPlan, DustCoin, DustCombineBatch, DustPlan, ProvenDustCoin,
};

pub(in crate::manager_cli::combine_market_cat_dust) fn proven_dust(
    coin_id: &str,
    amount: u64,
) -> ProvenDustCoin {
    let mut cat = crate::coinset::test_support::cat_with_amount(amount);
    cat.coin = chia_protocol::Coin::new(
        hex_to_bytes32(coin_id).expect("coin id"),
        cat.coin.puzzle_hash,
        amount,
    );
    ProvenDustCoin::from_cat(cat)
}

pub(in crate::manager_cli::combine_market_cat_dust) fn dust_combine_batch_from_ids(
    ids: &[u8],
) -> DustCombineBatch {
    DustCombineBatch {
        items: ids
            .iter()
            .map(|id| {
                let parent = format!("{id:064x}");
                proven_dust(&parent, 100)
            })
            .collect(),
    }
}

pub(in crate::manager_cli::combine_market_cat_dust) fn sample_combine_batch_plan() -> DustPlan {
    DustPlan {
        scan_dust_count: 4,
        batches: DustBatchPlan {
            combinable_batches: vec![
                dust_combine_batch_from_ids(&[1]),
                dust_combine_batch_from_ids(&[2]),
                dust_combine_batch_from_ids(&[3]),
            ],
            uncombinable: vec![DustCoin {
                coin_id: "f".repeat(64),
                amount: 1,
            }],
        },
        lineage_excluded: Vec::new(),
    }
}

pub(in crate::manager_cli::combine_market_cat_dust) fn ok_mixed_split_result() -> MixedSplitResult {
    MixedSplitResult {
        spend_bundle_hex: String::new(),
        broadcast_status: Some("submitted".to_string()),
        selected_coin_ids: vec!["aa".repeat(64)],
        offered_total: 200,
        target_total: 200,
        change_amount: 0,
    }
}
