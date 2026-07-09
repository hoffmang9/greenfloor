//! Single-scan market watch plan: local cancel-metadata heal + Dexie HTTP roles.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tracing::Level;

use crate::adapters::DexieClient;
use crate::cycle::ReconcileState;
use crate::error::SignerResult;
use crate::hex::normalize_hex_id;
use crate::offer::dexie_payload::extract_coin_ids_from_offer_payload;
use crate::operator_log::{LogContext, DEXIE_WATCHLIST_AUGMENT_ERROR};
use crate::storage::SqliteStore;

use super::dexie_size::offer_matches_local_id;
use super::reconcile_transition::ReconcileMarketCycleMetrics;

/// Per-market watch reconcile plan from one `offer_state` scan.
#[derive(Debug, Clone, Default)]
pub struct MarketWatchPlan {
    /// Dexie-authoritative watched offers (`publish_venue=dexie`).
    pub authoritative: HashSet<String>,
    /// NULL or dexie venue rows that still lack watches after local metadata heal.
    pub heal_only: HashSet<String>,
}

impl MarketWatchPlan {
    #[must_use]
    pub fn needs_dexie_http(&self) -> bool {
        !self.authoritative.is_empty() || !self.heal_only.is_empty()
    }
}

/// One scan: heal from cancel metadata, classify Dexie authoritative vs heal-only.
///
/// # Errors
///
/// Returns an error if `SQLite` reads/writes fail.
pub fn classify_and_heal_local(
    store: &SqliteStore,
    market_id: &str,
) -> SignerResult<MarketWatchPlan> {
    let clean_market = market_id.trim();
    if clean_market.is_empty() {
        return Ok(MarketWatchPlan::default());
    }
    let rows = store.list_offer_states(Some(clean_market), 5000)?;
    let mut plan = MarketWatchPlan::default();
    for row in rows {
        let Ok(state) = ReconcileState::parse(&row.state) else {
            continue;
        };
        if !state.is_watched_for_reconcile() || matches!(state, ReconcileState::CancelSubmitted) {
            continue;
        }
        let venue = row
            .publish_venue
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let is_dexie_auth = venue.is_some_and(|v| v.eq_ignore_ascii_case("dexie"));
        if is_dexie_auth {
            plan.authoritative.insert(row.offer_id.clone());
        }

        let mut has_watches = store.offer_has_coin_watches(&row.offer_id)?;
        if !has_watches {
            if let Some(meta) = store.offer_cancel_metadata_for_id(&row.offer_id)? {
                let mut coins = Vec::new();
                let mut p2s = Vec::new();
                if let Some(coin) = meta
                    .fields
                    .input_coin_id
                    .as_deref()
                    .map(normalize_hex_id)
                    .filter(|value| value.len() == 64)
                {
                    coins.push(coin);
                }
                if let Some(p2) = meta
                    .fields
                    .maker_puzzle_hash
                    .as_deref()
                    .map(normalize_hex_id)
                    .filter(|value| value.len() == 64)
                {
                    p2s.push(p2);
                }
                if !coins.is_empty() || !p2s.is_empty() {
                    store.ensure_offer_coin_watches(&row.offer_id, clean_market, &coins, &p2s)?;
                    has_watches = true;
                }
            }
        }
        if has_watches {
            continue;
        }
        // Still missing watches: Dexie payload heal for dexie + legacy NULL venue only.
        let may_need_dexie = match venue {
            None => true,
            Some(v) if v.eq_ignore_ascii_case("dexie") => true,
            Some(_) => false,
        };
        if may_need_dexie {
            plan.heal_only.insert(row.offer_id);
        }
    }
    Ok(plan)
}

/// Fetch Dexie payloads for heal-only ids and ensure coin watches. No lifecycle.
///
/// Uses the market list when present; otherwise `get_offer` per id.
///
/// # Errors
///
/// Returns an error if Dexie HTTP or `SQLite` writes fail.
pub async fn fetch_and_ensure_watches(
    dexie: &DexieClient,
    store: &SqliteStore,
    market_id: &str,
    heal_only: &HashSet<String>,
    list_offers: &[Value],
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<()> {
    if heal_only.is_empty() {
        return Ok(());
    }
    let mut by_local_id: HashMap<String, Value> = HashMap::default();
    for offer in list_offers {
        let Some(obj) = offer.as_object() else {
            continue;
        };
        for key in crate::daemon::dexie_size::dexie_offer_lookup_keys(obj) {
            if heal_only.contains(&key) {
                by_local_id.entry(key).or_insert_with(|| offer.clone());
            }
        }
    }
    for offer_id in heal_only {
        if by_local_id.contains_key(offer_id) {
            continue;
        }
        match dexie.get_offer(offer_id).await {
            Ok(response) => {
                if let Some(single) = response.body().get("offer") {
                    if offer_matches_local_id(single, offer_id) {
                        by_local_id.insert(offer_id.clone(), single.clone());
                    }
                }
            }
            Err(err) => {
                metrics.cycle_errors += 1;
                LogContext::MARKET_CYCLE.dual_audit(
                    store,
                    Level::WARN,
                    "dexie watch heal fetch failed",
                    DEXIE_WATCHLIST_AUGMENT_ERROR,
                    &serde_json::json!({
                        "market_id": market_id,
                        "offer_id": offer_id,
                        "error": err.to_string(),
                    }),
                    Some(market_id),
                )?;
            }
        }
    }
    for (offer_id, raw) in &by_local_id {
        if store.offer_has_coin_watches(offer_id)? {
            continue;
        }
        let coin_ids = extract_coin_ids_from_offer_payload(raw);
        if !coin_ids.is_empty() {
            store.ensure_offer_coin_watches(offer_id, market_id, &coin_ids, &[])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::{OfferExecutionMode, PresplitCancelFields};
    use crate::storage::OfferCancelWrite;
    use tempfile::tempdir;

    #[test]
    fn classify_heals_null_venue_from_cancel_metadata() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        let p2 = "ef".repeat(32);
        let fields = PresplitCancelFields {
            input_coin_id: Some(coin.clone()),
            fixed_delegated_puzzle_hash: Some("aa".repeat(32)),
            maker_puzzle_hash: Some(p2),
        };
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                None,
                &chrono::Utc::now().to_rfc3339(),
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    publish_venue: None,
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
        let plan = classify_and_heal_local(&store, "m1").expect("plan");
        assert!(plan.authoritative.is_empty());
        assert!(plan.heal_only.is_empty());
        assert!(store.offer_has_coin_watches(&offer_id).expect("healed"));
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("coins")
            .contains(&coin));
    }

    #[test]
    fn classify_skips_coinset_for_heal_only() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                None,
                &chrono::Utc::now().to_rfc3339(),
                OfferCancelWrite {
                    fields: Some(&PresplitCancelFields::default()),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    publish_venue: Some("coinset"),
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
        let plan = classify_and_heal_local(&store, "m1").expect("plan");
        assert!(plan.authoritative.is_empty());
        assert!(plan.heal_only.is_empty());
    }

    #[test]
    fn classify_null_venue_without_metadata_needs_dexie_heal() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                None,
                &chrono::Utc::now().to_rfc3339(),
                OfferCancelWrite {
                    publish_venue: None,
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
        let plan = classify_and_heal_local(&store, "m1").expect("plan");
        assert!(plan.authoritative.is_empty());
        assert_eq!(plan.heal_only, HashSet::from([offer_id]));
    }
}
