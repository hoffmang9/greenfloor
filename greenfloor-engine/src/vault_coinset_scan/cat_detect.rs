use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;

use crate::coinset::{
    child_cat_asset_ids_from_parent_spend, chunk_values, coin_from_record, coin_id_from_record,
    coin_spend_from_solution_payload, to_coinset_hex, u64_from_value, DirectCoinsetScanClient,
};
use crate::error::SignerResult;
use crate::hex::normalize_hex_id;
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
    #[must_use]
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

/// Classify coin rows.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn classify_coin_rows(
    scanner: &DirectCoinsetScanClient,
    rows: &mut HashMap<String, CoinRow>,
    nonce_to_p2: &HashMap<u32, String>,
    asset_id_to_symbols: &BTreeMap<String, Vec<String>>,
    parent_lookup_batch_size: u32,
    caches: &mut CatDetectCaches,
) -> SignerResult<()> {
    caches.parent_record_cache =
        prefetch_parent_records(scanner, rows, parent_lookup_batch_size).await?;

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
        let parent_key = normalize_hex_id(&row.parent_coin_info);
        pending_by_parent
            .entry(parent_key)
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

async fn prefetch_parent_records(
    scanner: &DirectCoinsetScanClient,
    rows: &HashMap<String, CoinRow>,
    parent_lookup_batch_size: u32,
) -> SignerResult<HashMap<String, Option<Value>>> {
    let unresolved_parent_ids: Vec<String> = rows
        .values()
        .filter_map(|row| {
            let parent_id = normalize_hex_id(&row.parent_coin_info);
            if parent_id.is_empty() {
                None
            } else {
                Some(parent_id)
            }
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut parent_record_cache: HashMap<String, Option<Value>> = unresolved_parent_ids
        .iter()
        .map(|parent_id| (parent_id.clone(), None))
        .collect();

    for parent_batch in chunk_values(&unresolved_parent_ids, parent_lookup_batch_size as usize) {
        let lookup_keys: Vec<String> = parent_batch
            .iter()
            .map(|parent_id| normalize_hex_id(parent_id))
            .filter(|parent_id| !parent_id.is_empty())
            .collect();
        let parent_records = scanner
            .by_names(
                &lookup_keys
                    .iter()
                    .filter_map(|parent_id| {
                        hex_to_bytes32(parent_id)
                            .ok()
                            .map(|bytes| to_coinset_hex(bytes.as_ref()))
                    })
                    .collect::<Vec<_>>(),
                true,
                None,
                None,
            )
            .await?;
        for lookup_key in lookup_keys {
            let matched = parent_records
                .iter()
                .find(|record| coin_id_from_record(record) == lookup_key);
            if let Some(record) = matched {
                parent_record_cache.insert(lookup_key, Some(record.clone()));
            }
        }
    }
    Ok(parent_record_cache)
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
        let cached_child_assets = lineage.child_asset_ids.clone();
        for child_id in child_ids {
            if let Some(row) = rows.get_mut(child_id) {
                if let Some(asset_id) = cached_child_assets.get(child_id) {
                    apply_cached_cat(row, asset_id, asset_id_to_symbols);
                } else {
                    mark_other(caches, rows, child_id);
                }
            }
        }
        return Ok(());
    }

    let parent_record = caches
        .parent_record_cache
        .get(parent_id)
        .cloned()
        .unwrap_or(None);

    let Some(parent_record) = parent_record else {
        fail_children(caches, rows, child_ids);
        return Ok(());
    };
    let Some(parent_coin) = coin_from_record(&parent_record) else {
        fail_children(caches, rows, child_ids);
        return Ok(());
    };
    let spent_height = u64_from_value(parent_record.get("spent_block_index"), 0);
    if spent_height == 0 {
        fail_children(caches, rows, child_ids);
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
        fail_children(caches, rows, child_ids);
        return Ok(());
    };
    let Some(parent_spend) = coin_spend_from_solution_payload(parent_coin, &solution) else {
        fail_children(caches, rows, child_ids);
        return Ok(());
    };

    let child_assets = match child_cat_asset_ids_from_parent_spend(parent_coin, &parent_spend) {
        Ok(child_assets) => child_assets,
        Err(err) => {
            tracing::warn!(
                parent_coin_id = parent_id,
                child_count = child_ids.len(),
                error = %err,
                "vault coinset scan: parent spend CAT parse failed; marking children as non-CAT"
            );
            fail_children(caches, rows, child_ids);
            return Ok(());
        }
    };
    for (child_id, asset_id) in &child_assets {
        caches
            .cat_asset_cache
            .insert(child_id.clone(), asset_id.clone());
    }
    caches.parent_lineage_cache.insert(
        parent_id.to_string(),
        ParentLineageEntry {
            spent_height,
            child_asset_ids: child_assets
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        },
    );
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

fn fail_children(
    caches: &mut CatDetectCaches,
    rows: &mut HashMap<String, CoinRow>,
    child_ids: &[String],
) {
    for child_id in child_ids {
        mark_other(caches, rows, child_id);
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

#[must_use]
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
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn lineage_cache_miss_writes_negative_cat_cache() {
        let parent_id = "a".repeat(64);
        let child_id = "b".repeat(64);
        let mut rows = HashMap::from([(
            child_id.clone(),
            CoinRow {
                coin_id: child_id.clone(),
                puzzle_hash: "c".repeat(64),
                parent_coin_info: parent_id.clone(),
                amount: 1000,
                confirmed_block_index: 1,
                spent_block_index: 0,
                discovered_nonces: vec![0],
                discovered_by_puzzle_hash: true,
                discovered_by_hint: false,
                kind: CoinKind::Unknown,
                cat_asset_id: None,
                cat_symbols: vec![],
            },
        )]);
        let mut caches = CatDetectCaches::new(
            HashMap::new(),
            HashMap::from([(
                parent_id.clone(),
                ParentLineageEntry {
                    spent_height: 10,
                    child_asset_ids: HashMap::new(),
                },
            )]),
        );
        for child_id in [child_id.as_str()] {
            if let Some(lineage) = caches.parent_lineage_cache.get(&parent_id) {
                if !lineage.child_asset_ids.contains_key(child_id) {
                    mark_other(&mut caches, &mut rows, child_id);
                }
            }
        }
        assert_eq!(rows[&child_id].kind, CoinKind::Other);
        assert_eq!(caches.cat_asset_cache.get(&child_id), Some(&String::new()));
    }

    #[test]
    fn classify_xch_when_puzzle_matches_member_hash() {
        let mut nonce_to_p2 = HashMap::new();
        let puzzle = "a".repeat(64);
        nonce_to_p2.insert(0, puzzle.clone());
        assert!(classify_xch_or_other(&puzzle, &nonce_to_p2, &[0]));
        assert!(!classify_xch_or_other(
            "b".repeat(64).as_str(),
            &nonce_to_p2,
            &[0]
        ));
    }
}
