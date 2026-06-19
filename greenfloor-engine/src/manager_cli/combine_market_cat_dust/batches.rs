use serde_json::{json, Value};

use crate::vault::mixed_split::MixedSplitResult;
use crate::vault_coinset_scan::{DustBatchPlan, DustCoin};

fn coin_ids_json(coins: &[DustCoin]) -> Value {
    json!(coins.iter().map(|coin| &coin.coin_id).collect::<Vec<_>>())
}

pub fn preview_batches_report(plan: &DustBatchPlan, can_combine: bool) -> Value {
    let mut entries = Vec::new();
    for batch in &plan.combinable_batches {
        entries.push(json!({
            "coin_ids": coin_ids_json(batch),
            "status": "preview",
            "would_combine": can_combine,
        }));
    }
    for coin in &plan.uncombinable {
        entries.push(orphan_batch_entry(coin));
    }
    json!(entries)
}

pub fn orphan_batch_entry(coin: &DustCoin) -> Value {
    json!({
        "coin_ids": coin_ids_json(std::slice::from_ref(coin)),
        "status": "orphan",
    })
}

pub fn executed_batch_entry(batch: &[DustCoin], result: &MixedSplitResult) -> Value {
    json!({
        "coin_ids": coin_ids_json(batch),
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

pub fn failed_batch_entry(batch: &[DustCoin], err: &str) -> Value {
    json!({
        "coin_ids": coin_ids_json(batch),
        "status": "failed",
        "exit_code": 1,
        "stderr_tail": err,
    })
}

pub fn append_orphan_entries(report: &mut Value, orphans: &[DustCoin]) {
    let Some(entries) = report.as_array_mut() else {
        return;
    };
    for coin in orphans {
        entries.push(orphan_batch_entry(coin));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_coinset_scan::{plan_dust_batches, DustCoin};

    #[test]
    fn preview_batches_report_uses_unified_schema() {
        let coins: Vec<DustCoin> = (0..3)
            .map(|i| DustCoin {
                coin_id: format!("{i:064x}"),
                amount: 1,
            })
            .collect();
        let plan = plan_dust_batches(&coins, 2);
        let report = preview_batches_report(&plan, true);
        let entries = report.as_array().expect("batch array");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("status"), Some(&json!("preview")));
        assert_eq!(entries[0].get("would_combine"), Some(&json!(true)));
        assert_eq!(entries[1].get("status"), Some(&json!("orphan")));
    }
}
