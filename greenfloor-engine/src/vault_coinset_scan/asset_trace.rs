//! Per-asset vault lineage: reception → intermediate coins → current balance.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;

use crate::hex::normalize_hex_id;
use crate::vault_coinset_scan::types::CoinRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetTraceRole {
    /// First vault-visible coin in a lineage branch (parent outside this asset set).
    Reception,
    /// Unspent coin contributing to current balance (not a reception root).
    Current,
    /// Spent with no descendants in this asset set (exited vault or terminal spend).
    Exit,
    /// Spent coin with descendants still in this asset set.
    Internal,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetTraceCoin {
    pub coin_id: String,
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_coin_id: Option<String>,
    pub child_coin_ids: Vec<String>,
    /// Other inputs spent in the same block/puzzle merge as this coin (excludes self).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub co_input_coin_ids: Vec<String>,
    pub role: AssetTraceRole,
    pub confirmed_block_index: u64,
    pub spent_block_index: u64,
    pub puzzle_hash: String,
    pub discovered_nonces: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetTraceBalance {
    pub unspent_coin_count: usize,
    pub unspent_amount_mojos: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetTraceChain {
    pub reception_coin_id: String,
    /// Coin ids from reception through terminal (inclusive).
    pub path: Vec<String>,
    pub terminal_role: AssetTraceRole,
    pub terminal_amount_mojos: u64,
}

/// Multiple inputs co-spent in one combine (same block + vault puzzle hash).
#[derive(Debug, Clone, Serialize)]
pub struct AssetTraceMerge {
    pub spent_block_index: u64,
    pub puzzle_hash: String,
    pub input_coin_ids: Vec<String>,
    pub output_coin_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetTraceResult {
    pub asset_id: String,
    pub asset_type: String,
    /// Parent-link tree plus same-block co-spend merge edges.
    pub lineage_model: &'static str,
    pub coin_count: usize,
    pub reception_count: usize,
    pub merge_count: usize,
    pub current_balance: AssetTraceBalance,
    pub coins: Vec<AssetTraceCoin>,
    pub chains: Vec<AssetTraceChain>,
    pub merges: Vec<AssetTraceMerge>,
}

struct NormalizedRow<'a> {
    coin_id: String,
    /// Normalized parent coin id; empty when absent.
    parent: String,
    puzzle_hash: String,
    row: &'a CoinRow,
}

/// Build per-asset lineage graph and chains from vault scan rows (already asset-filtered).
#[must_use]
pub fn build_asset_trace(asset_id: &str, asset_type: &str, rows: &[CoinRow]) -> AssetTraceResult {
    let normalized = normalize_rows(rows);
    let coin_ids: HashSet<String> = normalized
        .iter()
        .map(|entry| entry.coin_id.clone())
        .collect();
    let children_by_parent = build_children_index(&normalized, &coin_ids);
    let merges = build_merges(&normalized, &children_by_parent);

    let mut coins = Vec::with_capacity(normalized.len());
    let mut unspent_coin_count = 0usize;
    let mut unspent_amount_mojos = 0u64;

    for entry in &normalized {
        let is_reception = entry.parent.is_empty() || !coin_ids.contains(&entry.parent);
        let child_coin_ids = children_by_parent
            .get(&entry.coin_id)
            .cloned()
            .unwrap_or_default();
        let has_children = !child_coin_ids.is_empty();
        let is_unspent = entry.row.spent_block_index == 0;
        if is_unspent {
            unspent_coin_count += 1;
            unspent_amount_mojos = unspent_amount_mojos.saturating_add(entry.row.amount);
        }

        coins.push(AssetTraceCoin {
            coin_id: entry.coin_id.clone(),
            amount: entry.row.amount,
            parent_coin_id: (!entry.parent.is_empty()).then(|| entry.parent.clone()),
            child_coin_ids,
            co_input_coin_ids: co_inputs_for(&entry.coin_id, &merges),
            role: classify_role(is_reception, is_unspent, has_children),
            confirmed_block_index: entry.row.confirmed_block_index,
            spent_block_index: entry.row.spent_block_index,
            puzzle_hash: entry.row.puzzle_hash.clone(),
            discovered_nonces: entry.row.discovered_nonces.clone(),
        });
    }

    coins.sort_by(|left, right| {
        left.confirmed_block_index
            .cmp(&right.confirmed_block_index)
            .then_with(|| left.coin_id.cmp(&right.coin_id))
    });

    let (chains, reception_count) = build_chains(&coins);
    AssetTraceResult {
        asset_id: asset_id.to_string(),
        asset_type: asset_type.to_string(),
        lineage_model: "parent_tree_with_same_block_merge_edges",
        coin_count: coins.len(),
        reception_count,
        merge_count: merges.len(),
        current_balance: AssetTraceBalance {
            unspent_coin_count,
            unspent_amount_mojos,
        },
        coins,
        chains,
        merges,
    }
}

fn classify_role(is_reception: bool, is_unspent: bool, has_children: bool) -> AssetTraceRole {
    if is_reception {
        AssetTraceRole::Reception
    } else if is_unspent {
        AssetTraceRole::Current
    } else if has_children {
        AssetTraceRole::Internal
    } else {
        AssetTraceRole::Exit
    }
}

fn normalize_rows(rows: &[CoinRow]) -> Vec<NormalizedRow<'_>> {
    rows.iter()
        .filter_map(|row| {
            let coin_id = normalize_hex_id(&row.coin_id);
            (!coin_id.is_empty()).then(|| NormalizedRow {
                coin_id,
                parent: normalize_hex_id(&row.parent_coin_info),
                puzzle_hash: normalize_hex_id(&row.puzzle_hash),
                row,
            })
        })
        .collect()
}

fn build_children_index(
    normalized: &[NormalizedRow<'_>],
    coin_ids: &HashSet<String>,
) -> HashMap<String, Vec<String>> {
    let mut children_by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for entry in normalized {
        if entry.parent.is_empty() || !coin_ids.contains(&entry.parent) {
            continue;
        }
        children_by_parent
            .entry(entry.parent.clone())
            .or_default()
            .push(entry.coin_id.clone());
    }
    for children in children_by_parent.values_mut() {
        children.sort_unstable();
    }
    children_by_parent
}

fn build_merges(
    normalized: &[NormalizedRow<'_>],
    children_by_parent: &HashMap<String, Vec<String>>,
) -> Vec<AssetTraceMerge> {
    let mut spent_clusters: BTreeMap<(u64, String), Vec<String>> = BTreeMap::new();
    for entry in normalized {
        if entry.row.spent_block_index == 0 {
            continue;
        }
        spent_clusters
            .entry((entry.row.spent_block_index, entry.puzzle_hash.clone()))
            .or_default()
            .push(entry.coin_id.clone());
    }

    let mut merges = Vec::new();
    for ((spent_block_index, puzzle_hash), mut cluster) in spent_clusters {
        cluster.sort_unstable();
        if cluster.len() < 2 {
            continue;
        }
        let output_coin_ids: Vec<String> = cluster
            .iter()
            .filter_map(|input_id| children_by_parent.get(input_id))
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if output_coin_ids.is_empty() {
            continue;
        }
        merges.push(AssetTraceMerge {
            spent_block_index,
            puzzle_hash,
            input_coin_ids: cluster,
            output_coin_ids,
        });
    }

    merges.sort_by(|left, right| {
        left.spent_block_index
            .cmp(&right.spent_block_index)
            .then_with(|| left.input_coin_ids.cmp(&right.input_coin_ids))
    });
    merges
}

/// Co-spend siblings from the merge that contains `coin_id` (merges are the sole source).
fn co_inputs_for(coin_id: &str, merges: &[AssetTraceMerge]) -> Vec<String> {
    merges
        .iter()
        .find(|merge| merge.input_coin_ids.iter().any(|id| id == coin_id))
        .map(|merge| {
            merge
                .input_coin_ids
                .iter()
                .filter(|id| id.as_str() != coin_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn build_chains(coins: &[AssetTraceCoin]) -> (Vec<AssetTraceChain>, usize) {
    let mut reception_ids: Vec<&str> = coins
        .iter()
        .filter(|coin| coin.role == AssetTraceRole::Reception)
        .map(|coin| coin.coin_id.as_str())
        .collect();
    reception_ids.sort_unstable();
    reception_ids.dedup();

    let coins_by_id: HashMap<&str, &AssetTraceCoin> = coins
        .iter()
        .map(|coin| (coin.coin_id.as_str(), coin))
        .collect();

    let mut chains = Vec::new();
    let mut path = Vec::new();
    for reception_id in &reception_ids {
        walk_chain(reception_id, &mut path, &coins_by_id, &mut chains);
    }
    chains.sort_by(|left, right| {
        left.reception_coin_id
            .cmp(&right.reception_coin_id)
            .then_with(|| left.path.len().cmp(&right.path.len()))
            .then_with(|| left.path.cmp(&right.path))
    });

    (chains, reception_ids.len())
}

fn walk_chain(
    node: &str,
    path: &mut Vec<String>,
    coins_by_id: &HashMap<&str, &AssetTraceCoin>,
    chains: &mut Vec<AssetTraceChain>,
) {
    path.push(node.to_string());
    let children = coins_by_id
        .get(node)
        .map_or(&[][..], |coin| coin.child_coin_ids.as_slice());
    if children.is_empty() {
        if let Some(chain) = terminal_chain(path, coins_by_id) {
            chains.push(chain);
        }
    } else {
        for child in children {
            walk_chain(child, path, coins_by_id, chains);
        }
    }
    path.pop();
}

fn terminal_chain(
    path: &[String],
    coins_by_id: &HashMap<&str, &AssetTraceCoin>,
) -> Option<AssetTraceChain> {
    let terminal_id = path.last()?;
    let terminal = coins_by_id.get(terminal_id.as_str())?;
    Some(AssetTraceChain {
        reception_coin_id: path.first()?.clone(),
        path: path.to_vec(),
        terminal_role: terminal.role,
        terminal_amount_mojos: terminal.amount,
    })
}

#[cfg(test)]
mod tests {
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
}
