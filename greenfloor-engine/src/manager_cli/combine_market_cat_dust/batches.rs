use serde_json::{json, Value};

use crate::vault::mixed_split::MixedSplitResult;
use crate::vault_coinset_scan::{DustCoin, DustCombineBatch, DustPlan};

fn coin_ids_json(coins: &[DustCoin]) -> Value {
    json!(coins.iter().map(|coin| &coin.coin_id).collect::<Vec<_>>())
}

fn batch_coin_ids_json(batch: &DustCombineBatch) -> Value {
    json!(batch
        .items
        .iter()
        .map(|item| &item.dust.coin_id)
        .collect::<Vec<_>>())
}

fn single_coin_status_entry(coin: &DustCoin, status: &str) -> Value {
    json!({
        "coin_ids": coin_ids_json(std::slice::from_ref(coin)),
        "status": status,
    })
}

pub fn preview_batches_report(plan: &DustPlan, can_combine: bool) -> Value {
    let mut entries = Vec::new();
    for batch in &plan.batches.combinable_batches {
        entries.push(json!({
            "coin_ids": batch_coin_ids_json(batch),
            "status": "preview",
            "would_combine": can_combine,
        }));
    }
    for coin in &plan.batches.uncombinable {
        entries.push(single_coin_status_entry(coin, "orphan"));
    }
    for coin in &plan.lineage_excluded {
        entries.push(single_coin_status_entry(coin, "lineage_excluded"));
    }
    json!(entries)
}

pub fn executed_batch_entry(batch: &DustCombineBatch, result: &MixedSplitResult) -> Value {
    json!({
        "coin_ids": batch_coin_ids_json(batch),
        "status": "executed",
        "exit_code": 0,
        "payload": {
            "broadcast_status": result.broadcast_status,
            "selected_coin_ids": result.selected_coin_ids,
            "offered_total": result.offered_total,
            "target_total": result.target_total,
            "change_amount": result.change_amount,
        },
    })
}

pub fn failed_batch_entry(batch: &DustCombineBatch, err: &str) -> Value {
    json!({
        "coin_ids": batch_coin_ids_json(batch),
        "status": "failed",
        "exit_code": 1,
        "stderr_tail": err,
    })
}

pub fn append_status_entries(report: &mut Value, coins: &[DustCoin], status: &str) {
    let Some(entries) = report.as_array_mut() else {
        return;
    };
    for coin in coins {
        entries.push(single_coin_status_entry(coin, status));
    }
}

pub fn append_orphan_entries(report: &mut Value, orphans: &[DustCoin]) {
    append_status_entries(report, orphans, "orphan");
}

pub fn append_lineage_excluded_entries(report: &mut Value, coins: &[DustCoin]) {
    append_status_entries(report, coins, "lineage_excluded");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coinset::test_support::cat_with_amount;
    use crate::hex::{hex_to_bytes32, normalize_hex_id};
    use crate::vault_coinset_scan::{plan_dust_batches, DustBatchPlan, DustCoin, ProvenDustCoin};

    fn proven_dust(coin_id: &str, amount: u64) -> ProvenDustCoin {
        let mut cat = cat_with_amount(amount);
        cat.coin = chia_protocol::Coin::new(
            hex_to_bytes32(coin_id).expect("coin id"),
            cat.coin.puzzle_hash,
            amount,
        );
        let coin_id = normalize_hex_id(&hex::encode(cat.coin.coin_id()));
        ProvenDustCoin::new(DustCoin { coin_id, amount }, cat).expect("proven dust")
    }

    #[test]
    fn preview_batches_report_uses_unified_schema() {
        let proven: Vec<_> = (0..3)
            .map(|i| proven_dust(&format!("{i:064x}"), 1))
            .collect();
        let batches = plan_dust_batches(&proven, 2);
        let plan = DustPlan {
            scan_dust_count: 3,
            batches,
            lineage_excluded: Vec::new(),
        };
        let report = preview_batches_report(&plan, true);
        let entries = report.as_array().expect("batch array");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("status"), Some(&json!("preview")));
        assert_eq!(entries[0].get("would_combine"), Some(&json!(true)));
        assert_eq!(entries[1].get("status"), Some(&json!("orphan")));
    }

    #[test]
    fn preview_batches_report_includes_lineage_excluded_entries() {
        let plan = DustPlan {
            scan_dust_count: 1,
            batches: DustBatchPlan {
                combinable_batches: Vec::new(),
                uncombinable: Vec::new(),
            },
            lineage_excluded: vec![DustCoin {
                coin_id: "ab".repeat(32),
                amount: 100,
            }],
        };
        let report = preview_batches_report(&plan, false);
        let entries = report.as_array().expect("batch array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].get("status"), Some(&json!("lineage_excluded")));
    }
}
