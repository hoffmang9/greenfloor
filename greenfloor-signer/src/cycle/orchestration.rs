use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

pub const MARKET_CYCLE_PHASES: &[&str] = &[
    "reconcile",
    "inventory",
    "strategy",
    "cancel",
    "coin_ops",
];

const TRACKED_STALE_SWEEP_STATES: &[&str] = &["open", "refresh_due"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketBatchSelection {
    pub selected_market_ids: Vec<String>,
    pub consumed_immediate_requeues: Vec<String>,
    pub cursor: usize,
    pub immediate_requeue_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleSweepCandidate {
    pub market_id: String,
    pub offer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleSweepHit {
    pub market_id: String,
    pub offer_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleSweepProgress {
    pub checked_offer_count: usize,
    pub requeue_market_ids: Vec<String>,
    pub hits: Vec<StaleSweepHit>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OfferStateRow {
    pub market_id: String,
    pub offer_id: String,
    pub state: String,
}

pub fn enqueue_immediate_requeue(
    immediate_requeue_ids: &[String],
    market_id: &str,
) -> Vec<String> {
    let clean_market_id = market_id.trim();
    if clean_market_id.is_empty() {
        return immediate_requeue_ids.to_vec();
    }
    let mut deduped: Vec<String> = immediate_requeue_ids
        .iter()
        .filter(|mid| mid.as_str() != clean_market_id)
        .cloned()
        .collect();
    deduped.insert(0, clean_market_id.to_string());
    deduped
}

pub fn select_market_batch(
    enabled_market_ids: &[String],
    slot_count: usize,
    cursor: usize,
    immediate_requeue_ids: &[String],
) -> MarketBatchSelection {
    let mut enabled_ids: Vec<String> = Vec::new();
    let mut enabled_set: HashSet<String> = HashSet::new();
    for market_id in enabled_market_ids {
        let clean = market_id.trim();
        if clean.is_empty() || enabled_set.contains(clean) {
            continue;
        }
        enabled_set.insert(clean.to_string());
        enabled_ids.push(clean.to_string());
    }
    if enabled_ids.is_empty() {
        return MarketBatchSelection {
            selected_market_ids: Vec::new(),
            consumed_immediate_requeues: Vec::new(),
            cursor: 0,
            immediate_requeue_ids: Vec::new(),
        };
    }

    let max_slots = slot_count.max(1);
    if max_slots >= enabled_ids.len() {
        let retained_requeues: Vec<String> = immediate_requeue_ids
            .iter()
            .filter(|market_id| enabled_set.contains(market_id.as_str()))
            .cloned()
            .collect();
        return MarketBatchSelection {
            selected_market_ids: enabled_ids,
            consumed_immediate_requeues: Vec::new(),
            cursor,
            immediate_requeue_ids: retained_requeues,
        };
    }

    let enabled_lookup: HashSet<&str> = enabled_ids.iter().map(String::as_str).collect();
    let mut selected_ids: Vec<String> = Vec::new();
    let mut selected_set: BTreeSet<String> = BTreeSet::new();
    let mut retained_requeues: Vec<String> = Vec::new();
    let mut consumed_requeues: Vec<String> = Vec::new();

    for market_id in immediate_requeue_ids {
        if !enabled_lookup.contains(market_id.as_str()) {
            continue;
        }
        if selected_set.contains(market_id) {
            continue;
        }
        if selected_ids.len() < max_slots {
            selected_ids.push(market_id.clone());
            selected_set.insert(market_id.clone());
            consumed_requeues.push(market_id.clone());
        } else {
            retained_requeues.push(market_id.clone());
        }
    }

    let round_robin_slots = max_slots.saturating_sub(selected_ids.len());
    let mut next_cursor = cursor;
    if round_robin_slots > 0 {
        let total_enabled = enabled_ids.len();
        let start_idx = cursor % total_enabled;
        let mut last_rr_idx: Option<usize> = None;
        for step in 0..total_enabled {
            let idx = (start_idx + step) % total_enabled;
            let market_id = &enabled_ids[idx];
            if selected_set.contains(market_id) {
                continue;
            }
            selected_ids.push(market_id.clone());
            selected_set.insert(market_id.clone());
            last_rr_idx = Some(idx);
            if selected_ids.len() >= max_slots {
                break;
            }
        }
        if let Some(last_rr_idx) = last_rr_idx {
            next_cursor = (last_rr_idx + 1) % total_enabled;
        }
    }

    MarketBatchSelection {
        selected_market_ids: selected_ids,
        consumed_immediate_requeues: consumed_requeues,
        cursor: next_cursor,
        immediate_requeue_ids: retained_requeues,
    }
}

pub fn should_use_market_slot_dispatch(enabled_market_count: usize, slot_count: usize) -> bool {
    slot_count > 0 && enabled_market_count > slot_count
}

pub fn dedupe_sorted_market_ids(market_ids: &[String]) -> Vec<String> {
    let mut deduped: BTreeSet<String> = BTreeSet::new();
    for market_id in market_ids {
        let clean = market_id.trim();
        if !clean.is_empty() {
            deduped.insert(clean.to_string());
        }
    }
    deduped.into_iter().collect()
}

pub fn should_log_disabled_market(now_monotonic: f64, next_log_deadline: f64) -> bool {
    next_log_deadline <= now_monotonic
}

pub fn next_disabled_market_log_deadline(now_monotonic: f64, interval_seconds: u64) -> f64 {
    now_monotonic + interval_seconds as f64
}

pub fn should_try_cat_inventory_fallback(coinset_scan_empty: bool, base_asset: &str) -> bool {
    if !coinset_scan_empty {
        return false;
    }
    let normalized = base_asset.trim().to_ascii_lowercase();
    !matches!(normalized.as_str(), "xch" | "1" | "")
}

pub fn collect_stale_sweep_candidates(
    rows: &[OfferStateRow],
    enabled_market_ids: &[String],
    per_market_limit: usize,
) -> Vec<StaleSweepCandidate> {
    let enabled: HashSet<&str> = enabled_market_ids
        .iter()
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .collect();
    if enabled.is_empty() {
        return Vec::new();
    }
    let per_market_limit = per_market_limit.max(1);
    let mut offer_ids_by_market: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        let market_id = row.market_id.trim();
        if market_id.is_empty() || !enabled.contains(market_id) {
            continue;
        }
        let state = row.state.trim().to_ascii_lowercase();
        if !TRACKED_STALE_SWEEP_STATES.contains(&state.as_str()) {
            continue;
        }
        let offer_id = row.offer_id.trim();
        if offer_id.is_empty() {
            continue;
        }
        let market_offer_ids = offer_ids_by_market.entry(market_id.to_string()).or_default();
        if market_offer_ids.iter().any(|existing| existing == offer_id) {
            continue;
        }
        if market_offer_ids.len() >= per_market_limit {
            continue;
        }
        market_offer_ids.push(offer_id.to_string());
    }

    let mut candidates = Vec::new();
    for (market_id, offer_ids) in offer_ids_by_market {
        for offer_id in offer_ids {
            candidates.push(StaleSweepCandidate {
                market_id: market_id.clone(),
                offer_id,
            });
        }
    }
    candidates
}

pub fn classify_dexie_stale_offer_status(status: i64) -> Option<&'static str> {
    match status {
        4 => Some("tx_confirmed"),
        6 => Some("offer_expired"),
        _ => None,
    }
}

pub fn is_dexie_offer_missing_error_text(error_text: &str) -> bool {
    let normalized = error_text.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    (normalized.contains("dexie_get_offer_error") && normalized.contains("404"))
        || normalized.contains("dexie_http_error:404")
        || (normalized.contains("http error 404") && normalized.contains("not found"))
}

pub fn record_stale_sweep_check(
    progress: &StaleSweepProgress,
    hit: Option<StaleSweepHit>,
) -> StaleSweepProgress {
    let mut next = progress.clone();
    next.checked_offer_count += 1;
    if let Some(hit) = hit {
        if !next
            .requeue_market_ids
            .iter()
            .any(|market_id| market_id == &hit.market_id)
        {
            next.requeue_market_ids.push(hit.market_id.clone());
            next.requeue_market_ids.sort();
        }
        next.hits.push(hit);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_market_batch_prioritizes_immediate_requeue_then_round_robin() {
        let enabled = vec![
            "m1".to_string(),
            "m2".to_string(),
            "m3".to_string(),
            "m4".to_string(),
        ];
        let first = select_market_batch(&enabled, 2, 0, &["m3".to_string()]);
        assert_eq!(first.selected_market_ids, vec!["m3", "m1"]);
        assert_eq!(first.consumed_immediate_requeues, vec!["m3"]);
        assert!(first.immediate_requeue_ids.is_empty());

        let second = select_market_batch(
            &enabled,
            2,
            first.cursor,
            &first.immediate_requeue_ids,
        );
        assert_eq!(second.selected_market_ids, vec!["m2", "m3"]);
        assert!(second.consumed_immediate_requeues.is_empty());
    }

    #[test]
    fn enqueue_immediate_requeue_deduplicates_and_prepends() {
        let updated = enqueue_immediate_requeue(
            &["m2".to_string(), "m1".to_string()],
            "m2",
        );
        assert_eq!(updated, vec!["m2", "m1"]);
    }

    #[test]
    fn collect_stale_sweep_candidates_respects_limits() {
        let rows = vec![
            OfferStateRow {
                market_id: "m1".to_string(),
                offer_id: "o1".to_string(),
                state: "open".to_string(),
            },
            OfferStateRow {
                market_id: "m1".to_string(),
                offer_id: "o2".to_string(),
                state: "expired".to_string(),
            },
        ];
        let candidates = collect_stale_sweep_candidates(&rows, &["m1".to_string()], 3);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].offer_id, "o1");
    }

    #[test]
    fn classify_dexie_stale_offer_status_maps_known_codes() {
        assert_eq!(classify_dexie_stale_offer_status(6), Some("offer_expired"));
        assert_eq!(classify_dexie_stale_offer_status(4), Some("tx_confirmed"));
        assert_eq!(classify_dexie_stale_offer_status(0), None);
    }

    #[test]
    fn is_dexie_offer_missing_error_text_detects_404() {
        assert!(is_dexie_offer_missing_error_text(
            "HTTP Error 404: Not Found"
        ));
        assert!(!is_dexie_offer_missing_error_text("timeout"));
    }
}
