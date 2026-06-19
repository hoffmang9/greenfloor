use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::coinset::{
    child_cat_asset_ids_from_parent_spend, coin_from_record, coin_spend_from_solution_payload,
    to_coinset_hex, u64_from_value, DirectCoinsetScanClient,
};
use crate::error::SignerResult;
use crate::vault::members::hex_to_bytes32;
use crate::vault_coinset_scan::checkpoint::ParentLineageEntry;
use crate::vault_coinset_scan::types::{CoinKind, CoinRow};

pub struct CatDetectCaches {
    pub cat_asset_cache: HashMap<String, String>,
    pub parent_record_cache: HashMap<String, Option<Value>>,
    pub puzzle_solution_cache: HashMap<String, Option<Value>>,
    pub parent_lineage_cache: HashMap<String, ParentLineageEntry>,
}

impl CatDetectCaches {
    pub fn new(
        cat_asset_cache: HashMap<String, String>,
        parent_lineage_cache: HashMap<String, ParentLineageEntry>,
    ) -> Self {
        Self {
            cat_asset_cache,
            parent_record_cache: HashMap::new(),
            puzzle_solution_cache: HashMap::new(),
            parent_lineage_cache,
        }
    }
}

pub async fn classify_coin_rows(
    scanner: &DirectCoinsetScanClient,
    rows: &mut HashMap<String, CoinRow>,
    nonce_to_p2: &HashMap<u32, String>,
    asset_id_to_symbols: &BTreeMap<String, Vec<String>>,
    caches: &mut CatDetectCaches,
) -> SignerResult<()> {
    let mut pending_by_parent: HashMap<String, Vec<String>> = HashMap::new();

    for (coin_id, row) in rows.iter_mut() {
        if classify_xch_or_other(&row.puzzle_hash, nonce_to_p2, &row.discovered_nonces) {
            row.kind = CoinKind::Xch;
            continue;
        }
        if let Some(cached_asset_id) = caches.cat_asset_cache.get(coin_id) {
            apply_cached_cat(row, cached_asset_id, asset_id_to_symbols);
            continue;
        }
        if row.parent_coin_info.is_empty() {
            row.kind = CoinKind::Other;
            caches
                .cat_asset_cache
                .insert(coin_id.clone(), String::new());
            continue;
        }
        pending_by_parent
            .entry(row.parent_coin_info.clone())
            .or_default()
            .push(coin_id.clone());
    }

    for (parent_id, child_ids) in pending_by_parent {
        resolve_parent_children(
            scanner,
            caches,
            &parent_id,
            &child_ids,
            rows,
            asset_id_to_symbols,
        )
        .await?;
    }
    Ok(())
}

async fn resolve_parent_children(
    scanner: &DirectCoinsetScanClient,
    caches: &mut CatDetectCaches,
    parent_id: &str,
    child_ids: &[String],
    rows: &mut HashMap<String, CoinRow>,
    asset_id_to_symbols: &BTreeMap<String, Vec<String>>,
) -> SignerResult<()> {
    if let Some(lineage) = caches.parent_lineage_cache.get(parent_id) {
        for child_id in child_ids {
            if let Some(row) = rows.get_mut(child_id) {
                if let Some(asset_id) = lineage.child_asset_ids.get(child_id) {
                    apply_cached_cat(row, asset_id, asset_id_to_symbols);
                } else {
                    row.kind = CoinKind::Other;
                }
            }
        }
        return Ok(());
    }

    let parent_record = if let Some(cached) = caches.parent_record_cache.get(parent_id) {
        cached.clone()
    } else {
        let parent_bytes = hex_to_bytes32(parent_id).ok();
        let lookup = match parent_bytes {
            Some(bytes) => {
                scanner
                    .by_names(&[to_coinset_hex(bytes.as_ref())], true, None, None)
                    .await?
            }
            None => Vec::new(),
        };
        let parent_record = lookup.into_iter().next();
        caches
            .parent_record_cache
            .insert(parent_id.to_string(), parent_record.clone());
        parent_record
    };

    let Some(parent_record) = parent_record else {
        for child_id in child_ids {
            mark_other(caches, rows, child_id);
        }
        return Ok(());
    };
    let Some(parent_coin) = coin_from_record(&parent_record) else {
        for child_id in child_ids {
            mark_other(caches, rows, child_id);
        }
        return Ok(());
    };
    let spent_height = u64_from_value(parent_record.get("spent_block_index"), 0);
    if spent_height == 0 {
        for child_id in child_ids {
            mark_other(caches, rows, child_id);
        }
        return Ok(());
    }

    let parent_coin_name = hex::encode(parent_coin.coin_id());
    let solution_cache_key = format!("{parent_coin_name}:{spent_height}");
    let solution = if let Some(cached) = caches.puzzle_solution_cache.get(&solution_cache_key) {
        cached.clone()
    } else {
        let solution = scanner
            .puzzle_and_solution(
                &to_coinset_hex(parent_coin.coin_id().as_ref()),
                spent_height,
            )
            .await?;
        caches
            .puzzle_solution_cache
            .insert(solution_cache_key, solution.clone());
        solution
    };

    let Some(solution) = solution else {
        for child_id in child_ids {
            mark_other(caches, rows, child_id);
        }
        return Ok(());
    };
    let Some(parent_spend) = coin_spend_from_solution_payload(parent_coin, &solution) else {
        for child_id in child_ids {
            mark_other(caches, rows, child_id);
        }
        return Ok(());
    };

    let child_assets = child_cat_asset_ids_from_parent_spend(parent_coin, &parent_spend)?;
    caches.parent_lineage_cache.insert(
        parent_id.to_string(),
        ParentLineageEntry {
            spent_height,
            child_asset_ids: child_assets.clone(),
        },
    );
    for (child_id, asset_id) in &child_assets {
        caches
            .cat_asset_cache
            .insert(child_id.clone(), asset_id.clone());
    }
    for child_id in child_ids {
        if let Some(row) = rows.get_mut(child_id) {
            if let Some(asset_id) = child_assets.get(child_id) {
                apply_cached_cat(row, asset_id, asset_id_to_symbols);
            } else {
                mark_other(caches, rows, child_id);
            }
        }
    }
    Ok(())
}

fn apply_cached_cat(
    row: &mut CoinRow,
    cached_asset_id: &str,
    asset_id_to_symbols: &BTreeMap<String, Vec<String>>,
) {
    if cached_asset_id.is_empty() {
        row.kind = CoinKind::Other;
        row.cat_asset_id = None;
        row.cat_symbols.clear();
    } else {
        row.kind = CoinKind::Cat;
        row.cat_asset_id = Some(cached_asset_id.to_string());
        row.cat_symbols = asset_id_to_symbols
            .get(cached_asset_id)
            .cloned()
            .unwrap_or_default();
    }
}

fn mark_other(caches: &mut CatDetectCaches, rows: &mut HashMap<String, CoinRow>, child_id: &str) {
    if let Some(row) = rows.get_mut(child_id) {
        row.kind = CoinKind::Other;
    }
    caches
        .cat_asset_cache
        .insert(child_id.to_string(), String::new());
}

pub fn classify_xch_or_other(
    row_puzzle_hash: &str,
    nonce_to_p2: &HashMap<u32, String>,
    discovered_nonces: &[u32],
) -> bool {
    let p2_hashes: HashSet<&str> = discovered_nonces
        .iter()
        .filter_map(|nonce| nonce_to_p2.get(nonce).map(String::as_str))
        .collect();
    !row_puzzle_hash.is_empty() && p2_hashes.contains(row_puzzle_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_xch_when_puzzle_matches_member_hash() {
        let mut nonce_to_p2 = HashMap::new();
        let puzzle = "aa".repeat(64);
        nonce_to_p2.insert(0, puzzle.clone());
        assert!(classify_xch_or_other(&puzzle, &nonce_to_p2, &[0]));
        assert!(!classify_xch_or_other(
            "bb".repeat(64).as_str(),
            &nonce_to_p2,
            &[0]
        ));
    }
}
