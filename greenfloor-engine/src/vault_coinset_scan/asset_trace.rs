//! Per-asset vault lineage: reception → intermediate coins → current balance.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

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
    pub role: AssetTraceRole,
    pub confirmed_block_index: u64,
    pub spent_block_index: u64,
    /// Canonical 64-char lowercase hex (same normalization as merge clustering).
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
    /// Coin ids from reception through terminal (inclusive). Reception is `path[0]`.
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

/// Lineage graph for one asset. JSON field names match manager `vault-asset-trace` output
/// (`resolved_asset_id`, derived counts) so the CLI can flatten this struct directly.
#[derive(Debug, Clone)]
pub struct AssetTraceResult {
    pub asset_id: String,
    pub asset_type: String,
    pub lineage_model: &'static str,
    pub current_balance: AssetTraceBalance,
    pub coins: Vec<AssetTraceCoin>,
    pub chains: Vec<AssetTraceChain>,
    pub merges: Vec<AssetTraceMerge>,
}

impl AssetTraceResult {
    #[must_use]
    pub fn reception_count(&self) -> usize {
        self.coins
            .iter()
            .filter(|coin| coin.role == AssetTraceRole::Reception)
            .count()
    }
}

impl Serialize for AssetTraceResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("AssetTraceResult", 10)?;
        state.serialize_field("resolved_asset_id", &self.asset_id)?;
        state.serialize_field("asset_type", &self.asset_type)?;
        state.serialize_field("lineage_model", &self.lineage_model)?;
        state.serialize_field("current_balance", &self.current_balance)?;
        state.serialize_field("reception_count", &self.reception_count())?;
        state.serialize_field("merge_count", &self.merges.len())?;
        state.serialize_field("lineage_coin_count", &self.coins.len())?;
        state.serialize_field("coins", &self.coins)?;
        state.serialize_field("chains", &self.chains)?;
        state.serialize_field("merges", &self.merges)?;
        state.end()
    }
}

struct NormalizedRow<'a> {
    coin_id: String,
    /// Normalized parent coin id; empty when absent.
    parent: String,
    puzzle_hash: String,
    row: &'a CoinRow,
}

fn parent_in_set(parent: &str, coin_ids: &HashSet<&str>) -> bool {
    !parent.is_empty() && coin_ids.contains(parent)
}

/// Build per-asset lineage graph and chains from vault scan rows (already asset-filtered).
#[must_use]
pub fn build_asset_trace(asset_id: &str, asset_type: &str, rows: &[CoinRow]) -> AssetTraceResult {
    let normalized = normalize_rows(rows);
    let coin_ids: HashSet<&str> = normalized.iter().map(|e| e.coin_id.as_str()).collect();
    let mut children_by_parent = build_children_index(&normalized, &coin_ids);
    let merges = build_merges(&normalized, &children_by_parent);

    let mut coins = Vec::with_capacity(normalized.len());
    let mut unspent_coin_count = 0usize;
    let mut unspent_amount_mojos = 0u64;

    for entry in &normalized {
        let child_coin_ids = children_by_parent
            .remove(entry.coin_id.as_str())
            .unwrap_or_default();
        let is_unspent = entry.row.spent_block_index == 0;
        if is_unspent {
            unspent_coin_count += 1;
            unspent_amount_mojos = unspent_amount_mojos.saturating_add(entry.row.amount);
        }
        let role = classify_role(
            !parent_in_set(&entry.parent, &coin_ids),
            is_unspent,
            !child_coin_ids.is_empty(),
        );

        coins.push(AssetTraceCoin {
            coin_id: entry.coin_id.clone(),
            amount: entry.row.amount,
            parent_coin_id: (!entry.parent.is_empty()).then(|| entry.parent.clone()),
            child_coin_ids,
            role,
            confirmed_block_index: entry.row.confirmed_block_index,
            spent_block_index: entry.row.spent_block_index,
            puzzle_hash: entry.puzzle_hash.clone(),
            discovered_nonces: entry.row.discovered_nonces.clone(),
        });
    }

    coins.sort_by(|left, right| {
        left.confirmed_block_index
            .cmp(&right.confirmed_block_index)
            .then_with(|| left.coin_id.cmp(&right.coin_id))
    });

    let chains = build_chains(&coins);
    AssetTraceResult {
        asset_id: asset_id.to_string(),
        asset_type: asset_type.to_string(),
        lineage_model: "parent_tree_with_same_block_merge_edges",
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

fn build_children_index<'a>(
    normalized: &'a [NormalizedRow<'_>],
    coin_ids: &HashSet<&str>,
) -> HashMap<&'a str, Vec<String>> {
    let mut children_by_parent: HashMap<&str, Vec<String>> = HashMap::new();
    for entry in normalized {
        if !parent_in_set(&entry.parent, coin_ids) {
            continue;
        }
        children_by_parent
            .entry(entry.parent.as_str())
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
    children_by_parent: &HashMap<&str, Vec<String>>,
) -> Vec<AssetTraceMerge> {
    let mut spent_clusters: BTreeMap<(u64, &str), Vec<&str>> = BTreeMap::new();
    for entry in normalized {
        if entry.row.spent_block_index == 0 {
            continue;
        }
        spent_clusters
            .entry((entry.row.spent_block_index, entry.puzzle_hash.as_str()))
            .or_default()
            .push(entry.coin_id.as_str());
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
            puzzle_hash: puzzle_hash.to_string(),
            input_coin_ids: cluster.into_iter().map(str::to_string).collect(),
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

fn build_chains(coins: &[AssetTraceCoin]) -> Vec<AssetTraceChain> {
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
    let mut visited = HashSet::new();
    for reception_id in &reception_ids {
        walk_chain(
            reception_id,
            &mut path,
            &coins_by_id,
            &mut chains,
            &mut visited,
        );
    }
    chains.sort_by(|left, right| {
        left.path
            .first()
            .cmp(&right.path.first())
            .then_with(|| left.path.len().cmp(&right.path.len()))
            .then_with(|| left.path.cmp(&right.path))
    });
    chains
}

fn walk_chain(
    node: &str,
    path: &mut Vec<String>,
    coins_by_id: &HashMap<&str, &AssetTraceCoin>,
    chains: &mut Vec<AssetTraceChain>,
    visited: &mut HashSet<String>,
) {
    if !visited.insert(node.to_string()) {
        chains.push(AssetTraceChain {
            path: path.clone(),
            terminal_role: AssetTraceRole::Internal,
            terminal_amount_mojos: coins_by_id.get(node).map_or(0, |coin| coin.amount),
        });
        return;
    }
    let Some(coin) = coins_by_id.get(node) else {
        visited.remove(node);
        return;
    };
    path.push(node.to_string());
    if coin.child_coin_ids.is_empty() {
        chains.push(AssetTraceChain {
            path: path.clone(),
            terminal_role: coin.role,
            terminal_amount_mojos: coin.amount,
        });
    } else {
        for child in &coin.child_coin_ids {
            walk_chain(child, path, coins_by_id, chains, visited);
        }
    }
    path.pop();
    visited.remove(node);
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

    fn coin<'a>(trace: &'a AssetTraceResult, coin_id: &str) -> &'a AssetTraceCoin {
        trace
            .coins
            .iter()
            .find(|coin| coin.coin_id == coin_id)
            .unwrap_or_else(|| panic!("missing coin {coin_id}"))
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
        assert_eq!(trace.reception_count(), 1);
        assert_eq!(trace.current_balance.unspent_coin_count, 2);
        assert_eq!(trace.current_balance.unspent_amount_mojos, 1000);
        assert_eq!(trace.chains.len(), 2);
        assert!(trace.merges.is_empty());

        let reception_coin = coin(&trace, &reception);
        assert_eq!(reception_coin.role, AssetTraceRole::Reception);
        assert_eq!(
            reception_coin.child_coin_ids,
            vec![split_a.clone(), split_b.clone()]
        );
        assert_eq!(coin(&trace, &split_a).role, AssetTraceRole::Current);
        assert_eq!(coin(&trace, &split_b).role, AssetTraceRole::Internal);
        assert_eq!(trace.chains[0].path[0], reception);
        assert_eq!(trace.chains[1].path[0], reception);
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
        assert_eq!(coin(&trace, &exit).role, AssetTraceRole::Exit);
    }

    #[test]
    fn trace_treats_missing_parent_in_set_as_reception_and_counts_unspent_balance() {
        let solo = id(1);
        let external = id(0xee);
        let rows = vec![row(&solo, &external, 100, 0, 5, &puzzle(1))];
        let trace = build_asset_trace("xch", "xch", &rows);
        assert_eq!(trace.reception_count(), 1);
        assert_eq!(trace.coins[0].role, AssetTraceRole::Reception);
        assert_eq!(
            trace.coins[0].parent_coin_id.as_deref(),
            Some(external.as_str())
        );
        assert_eq!(trace.coins[0].role, trace.chains[0].terminal_role);
        assert_eq!(trace.current_balance.unspent_coin_count, 1);
        assert_eq!(trace.current_balance.unspent_amount_mojos, 100);
        assert_eq!(trace.chains[0].path.as_slice(), [solo.as_str()]);
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
        assert_eq!(trace.merges.len(), 1);
        assert_eq!(
            trace.merges[0].input_coin_ids,
            vec![input_a.clone(), input_b.clone()]
        );
        assert_eq!(trace.merges[0].output_coin_ids, vec![combined]);
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
        assert!(trace.merges.is_empty());
    }

    #[test]
    fn trace_normalizes_puzzle_hash_on_coins_and_merges() {
        let input_a = id(1);
        let input_b = id(2);
        let combined = id(3);
        let external = id(0xee);
        let ph = puzzle(0x22);
        let prefixed = format!("0x{ph}");
        let rows = vec![
            row(&input_a, &external, 1000, 50, 1, &prefixed),
            row(&input_b, &external, 2000, 50, 2, &prefixed),
            row(&combined, &input_a, 3000, 0, 51, &prefixed),
        ];
        let trace = build_asset_trace("aa".repeat(64).as_str(), "cat", &rows);
        assert_eq!(coin(&trace, &input_a).puzzle_hash, ph);
        assert_eq!(trace.merges[0].puzzle_hash, ph);
    }

    #[test]
    fn trace_json_shape_matches_manager_lineage_contract() {
        let trace = build_asset_trace("xch", "xch", &[]);
        let payload = serde_json::to_value(&trace).expect("json");
        assert_eq!(
            payload.get("resolved_asset_id"),
            Some(&serde_json::json!("xch"))
        );
        assert_eq!(
            payload.get("lineage_model"),
            Some(&serde_json::json!(
                "parent_tree_with_same_block_merge_edges"
            ))
        );
        assert_eq!(
            payload.get("lineage_coin_count"),
            Some(&serde_json::json!(0))
        );
        assert_eq!(payload.get("merge_count"), Some(&serde_json::json!(0)));
        assert_eq!(payload.get("reception_count"), Some(&serde_json::json!(0)));
        assert!(payload.get("coin_count").is_none());
        assert!(payload
            .get("merges")
            .and_then(serde_json::Value::as_array)
            .is_some());
    }
}
