use super::*;
use crate::vault_coinset_scan::types::{CoinKind, CoinRow};

fn row(
    coin_id: &str,
    parent: &str,
    amount: u64,
    spent: u64,
    confirmed: u64,
    puzzle_hash: &str,
) -> CoinRow {
    CoinRow {
        coin_id: coin_id.to_string(),
        puzzle_hash: puzzle_hash.to_string(),
        parent_coin_info: parent.to_string(),
        amount,
        confirmed_block_index: confirmed,
        spent_block_index: spent,
        discovered_nonces: vec![0],
        discovered_by_puzzle_hash: true,
        discovered_by_hint: false,
        kind: CoinKind::Cat,
        cat_asset_id: Some("aa".repeat(64)),
        cat_symbols: vec!["TEST".to_string()],
    }
}

fn id(byte: u8) -> String {
    format!("{byte:064x}")
}

fn puzzle(byte: u8) -> String {
    format!("{byte:064x}")
}

#[test]
fn trace_identifies_reception_split_and_current_leaves() {
    let reception = id(1);
    let split_a = id(2);
    let split_b = id(3);
    let exit_child = id(4);
    let external = id(0xee);
    let ph = puzzle(0x11);
    let rows = vec![
        row(&reception, &external, 1000, 100, 1, &ph),
        row(&split_a, &reception, 600, 0, 101, &ph),
        row(&split_b, &reception, 400, 200, 101, &ph),
        row(&exit_child, &split_b, 400, 0, 201, &ph),
    ];
    let trace = build_asset_trace("aa".repeat(64).as_str(), "cat", &rows);
    assert_eq!(trace.reception_count, 1);
    assert_eq!(trace.current_balance.unspent_coin_count, 2);
    assert_eq!(trace.current_balance.unspent_amount_mojos, 1000);
    assert_eq!(trace.chains.len(), 2);
    assert_eq!(trace.merge_count, 0);

    let reception_coin = trace
        .coins
        .iter()
        .find(|coin| coin.coin_id == reception)
        .expect("reception");
    assert_eq!(reception_coin.role, AssetTraceRole::Reception);
    assert_eq!(
        reception_coin.child_coin_ids,
        vec![split_a.clone(), split_b.clone()]
    );

    let split_a_coin = trace
        .coins
        .iter()
        .find(|coin| coin.coin_id == split_a)
        .expect("split_a");
    assert_eq!(split_a_coin.role, AssetTraceRole::Current);

    let split_b_row = trace
        .coins
        .iter()
        .find(|coin| coin.coin_id == split_b)
        .expect("split_b");
    assert_eq!(split_b_row.role, AssetTraceRole::Internal);
}

#[test]
fn trace_marks_exit_when_spent_without_vault_children() {
    let reception = id(1);
    let exit = id(2);
    let external = id(0xee);
    let rows = vec![
        row(&reception, &external, 500, 50, 1, &puzzle(1)),
        row(&exit, &reception, 500, 51, 2, &puzzle(1)),
    ];
    let trace = build_asset_trace("aa".repeat(64).as_str(), "cat", &rows);
    assert_eq!(trace.current_balance.unspent_coin_count, 0);
    assert_eq!(trace.chains.len(), 1);
    assert_eq!(trace.chains[0].terminal_role, AssetTraceRole::Exit);
    let exit_coin = trace
        .coins
        .iter()
        .find(|coin| coin.coin_id == exit)
        .expect("exit");
    assert_eq!(exit_coin.role, trace.chains[0].terminal_role);
}

#[test]
fn trace_treats_missing_parent_in_set_as_reception() {
    let solo = id(1);
    let external = id(0xee);
    let rows = vec![row(&solo, &external, 100, 0, 5, &puzzle(1))];
    let trace = build_asset_trace("xch", "xch", &rows);
    assert_eq!(trace.reception_count, 1);
    assert_eq!(trace.coins[0].role, AssetTraceRole::Reception);
    assert_eq!(
        trace.coins[0].parent_coin_id.as_deref(),
        Some(external.as_str())
    );
    assert_eq!(trace.coins[0].role, trace.chains[0].terminal_role);
}

#[test]
fn trace_counts_unspent_reception_toward_current_balance() {
    let solo = id(1);
    let external = id(0xee);
    let rows = vec![row(&solo, &external, 100, 0, 5, &puzzle(1))];
    let trace = build_asset_trace("xch", "xch", &rows);
    assert_eq!(trace.current_balance.unspent_coin_count, 1);
    assert_eq!(trace.current_balance.unspent_amount_mojos, 100);
    assert_eq!(trace.chains[0].terminal_role, AssetTraceRole::Reception);
}

#[test]
fn trace_records_same_block_combine_merge() {
    let input_a = id(1);
    let input_b = id(2);
    let combined = id(3);
    let external = id(0xee);
    let ph = puzzle(0x22);
    let rows = vec![
        row(&input_a, &external, 1000, 50, 1, &ph),
        row(&input_b, &external, 2000, 50, 2, &ph),
        row(&combined, &input_a, 3000, 0, 51, &ph),
    ];
    let trace = build_asset_trace("aa".repeat(64).as_str(), "cat", &rows);
    assert_eq!(trace.merge_count, 1);
    assert_eq!(
        trace.merges[0].input_coin_ids,
        vec![input_a.clone(), input_b]
    );
    assert_eq!(trace.merges[0].output_coin_ids, vec![combined.clone()]);
    let input_a_coin = trace
        .coins
        .iter()
        .find(|coin| coin.coin_id == input_a)
        .expect("input_a");
    assert_eq!(input_a_coin.co_input_coin_ids, vec![id(2)]);
    assert_eq!(combined, trace.merges[0].output_coin_ids[0]);
}

#[test]
fn trace_skips_merge_when_same_block_spends_have_no_outputs_in_scan() {
    let input_a = id(1);
    let input_b = id(2);
    let external = id(0xee);
    let ph = puzzle(0x33);
    let rows = vec![
        row(&input_a, &external, 1000, 50, 1, &ph),
        row(&input_b, &external, 2000, 50, 2, &ph),
    ];
    let trace = build_asset_trace("aa".repeat(64).as_str(), "cat", &rows);
    assert_eq!(trace.merge_count, 0);
    assert!(trace.merges.is_empty());
    let input_a_coin = trace
        .coins
        .iter()
        .find(|coin| coin.coin_id == input_a)
        .expect("input_a");
    assert!(input_a_coin.co_input_coin_ids.is_empty());
}

#[test]
fn trace_emits_manager_json_shape_smoke() {
    let trace = build_asset_trace("xch", "xch", &[]);
    let payload = serde_json::to_value(&trace).expect("json");
    assert_eq!(
        payload.get("lineage_model"),
        Some(&serde_json::json!(
            "parent_tree_with_same_block_merge_edges"
        ))
    );
    assert!(payload
        .get("merges")
        .and_then(serde_json::Value::as_array)
        .is_some());
}
