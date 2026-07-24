//! Per-asset vault lineage: reception → intermediate coins → current balance.

#[cfg(test)]
mod tests;

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
    let merge_by_input = merge_index_by_input(&merges);

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
            co_input_coin_ids: co_inputs_from_merge(&entry.coin_id, &merges, &merge_by_input),
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

/// Map each merge input coin id to its merge index (single source: `merges`).
fn merge_index_by_input(merges: &[AssetTraceMerge]) -> HashMap<&str, usize> {
    let mut index = HashMap::new();
    for (merge_idx, merge) in merges.iter().enumerate() {
        for input_id in &merge.input_coin_ids {
            index.insert(input_id.as_str(), merge_idx);
        }
    }
    index
}

fn co_inputs_from_merge(
    coin_id: &str,
    merges: &[AssetTraceMerge],
    merge_by_input: &HashMap<&str, usize>,
) -> Vec<String> {
    let Some(&merge_idx) = merge_by_input.get(coin_id) else {
        return Vec::new();
    };
    merges[merge_idx]
        .input_coin_ids
        .iter()
        .filter(|id| id.as_str() != coin_id)
        .cloned()
        .collect()
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
        .map(|coin| coin.child_coin_ids.as_slice())
        .unwrap_or(&[]);
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
