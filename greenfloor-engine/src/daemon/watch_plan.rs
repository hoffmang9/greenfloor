//! Single-scan market watch plan: local cancel-metadata heal + Dexie HTTP roles.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tracing::Level;

use crate::adapters::DexieClient;
use crate::coinset::extract_maker_watch_keys_from_offer_text;
use crate::cycle::ReconcileState;
use crate::error::SignerResult;
use crate::hex::normalize_hex_id;
use crate::offer::dexie_payload::{extract_coin_ids_from_offer_payload, DexieOfferPayload};
use crate::operator_log::{LogContext, DEXIE_WATCHLIST_AUGMENT_ERROR};
use crate::storage::SqliteStore;

use super::dexie_size::offer_matches_local_id;
use super::reconcile_transition::ReconcileMarketCycleMetrics;
use crate::storage::OfferStateListRow;

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

/// One scan: heal from cancel metadata, classify Dexie roles, and collect
/// `cancel_submitted` rows for reconcile orphan policy (Dexie HTTP never drives
/// that state).
///
/// # Errors
///
/// Returns an error if `SQLite` reads/writes fail.
pub fn classify_and_heal_local(
    store: &SqliteStore,
    market_id: &str,
) -> SignerResult<(MarketWatchPlan, Vec<OfferStateListRow>)> {
    let clean_market = market_id.trim();
    if clean_market.is_empty() {
        return Ok((MarketWatchPlan::default(), Vec::new()));
    }
    let rows = store.list_offer_states(Some(clean_market), 5000)?;
    let mut plan = MarketWatchPlan::default();
    let mut cancel_submitted_rows: Vec<OfferStateListRow> = Vec::new();
    for row in rows {
        let Ok(state) = ReconcileState::parse(&row.state) else {
            continue;
        };
        if matches!(state, ReconcileState::CancelSubmitted) {
            cancel_submitted_rows.push(row);
            continue;
        }
        if !state.is_watched_for_reconcile() {
            continue;
        }
        let venue = crate::config::Venue::parse_optional(row.publish_venue.as_deref());
        let is_dexie_auth = venue.is_some_and(crate::config::Venue::is_dexie);
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
            Some(v) => v.is_dexie(),
        };
        if may_need_dexie {
            plan.heal_only.insert(row.offer_id);
        }
    }
    Ok((plan, cancel_submitted_rows))
}

/// Maker coin ids + on-chain p2s for durable watch heal from a Dexie payload.
///
/// Prefers decoding the `offer1…` file (cancellable inputs). Falls back to JSON
/// coin-id walk when the offer string is absent or undecodable.
#[must_use]
fn maker_watch_keys_from_dexie_payload(raw: &Value) -> (Vec<String>, Vec<String>) {
    let payload = DexieOfferPayload::new(raw.clone());
    if let Some(text) = payload.offer_file_text() {
        if let Ok((coins, p2s)) = extract_maker_watch_keys_from_offer_text(text) {
            if !coins.is_empty() || !p2s.is_empty() {
                return (coins, p2s);
            }
        }
    }
    (extract_coin_ids_from_offer_payload(raw), Vec::new())
}

#[must_use]
fn maker_p2s_present(raw: &Value) -> bool {
    !maker_watch_keys_from_dexie_payload(raw).1.is_empty()
}

async fn fetch_dexie_offer_body(
    dexie: &DexieClient,
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<Option<Value>> {
    match dexie.get_offer(offer_id).await {
        Ok(response) => {
            if let Some(single) = response.body().get("offer") {
                if offer_matches_local_id(single, offer_id) {
                    return Ok(Some(single.clone()));
                }
            }
            Ok(None)
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
            Ok(None)
        }
    }
}

/// Fetch Dexie payloads for heal-only ids and ensure coin + maker p2 watches. No lifecycle.
///
/// Uses the market list when present. When a list row lacks maker p2s (no
/// decodable `offer1…`), calls `get_offer` so heal is not stuck coin-only.
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
        if store.offer_has_coin_watches(offer_id)? {
            continue;
        }
        let list_raw = by_local_id.get(offer_id).cloned();
        let needs_offer_file = list_raw.as_ref().is_none_or(|raw| !maker_p2s_present(raw));
        let raw = if needs_offer_file {
            match fetch_dexie_offer_body(dexie, store, market_id, offer_id, metrics).await? {
                Some(single) => Some(single),
                None => list_raw,
            }
        } else {
            list_raw
        };
        let Some(raw) = raw else {
            continue;
        };
        let (coin_ids, p2s) = maker_watch_keys_from_dexie_payload(&raw);
        if !coin_ids.is_empty() || !p2s.is_empty() {
            store.ensure_offer_coin_watches(offer_id, market_id, &coin_ids, &p2s)?;
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
        let (plan, _) = classify_and_heal_local(&store, "m1").expect("plan");
        assert!(plan.authoritative.is_empty());
        assert!(plan.heal_only.is_empty());
        assert!(store.offer_has_coin_watches(&offer_id).expect("healed"));
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("coins")
            .contains(&coin));
        assert!(store
            .list_watched_p2s_for_market("m1")
            .expect("p2s")
            .contains(&"ef".repeat(32)));
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
        let (plan, _) = classify_and_heal_local(&store, "m1").expect("plan");
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
        let (plan, _) = classify_and_heal_local(&store, "m1").expect("plan");
        assert!(plan.authoritative.is_empty());
        assert_eq!(plan.heal_only, HashSet::from([offer_id]));
    }

    #[test]
    fn maker_watch_keys_from_dexie_payload_falls_back_to_json_coin_ids() {
        let coin = "b".repeat(64);
        let payload = serde_json::json!({"offer": {"coin_id": coin}});
        let (coins, p2s) = maker_watch_keys_from_dexie_payload(&payload);
        assert_eq!(coins, vec![coin]);
        assert!(p2s.is_empty());
        assert!(!maker_p2s_present(&payload));
    }

    #[test]
    fn maker_p2s_present_false_without_offer_file() {
        let payload = serde_json::json!({"id": "ab".repeat(32), "coin_id": "cd".repeat(32)});
        assert!(!maker_p2s_present(&payload));
    }

    #[test]
    fn classify_past_grace_cancel_submitted_resets_to_open() {
        use crate::offer::lifecycle::{apply_cancel_submitted_rows, ReconcilePersistOptions};

        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        let cancel_tx = "cd".repeat(32);
        let submitted = (chrono::Utc::now() - chrono::Duration::seconds(600)).to_rfc3339();
        store
            .prepare_offer_cancel_submitted_at(&offer_id, "m1", &cancel_tx, None, &submitted)
            .expect("prepare");
        let (plan, cancel_rows) = classify_and_heal_local(&store, "m1").expect("plan");
        assert!(plan.authoritative.is_empty());
        assert!(plan.heal_only.is_empty());
        assert_eq!(cancel_rows.len(), 1);
        apply_cancel_submitted_rows(
            &store,
            &cancel_rows,
            &ReconcilePersistOptions {
                action: "cancel_submitted_orphan_reconcile",
                venue: None,
                dexie_error: None,
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "open");
    }

    #[test]
    fn classify_within_grace_preserves_cancel_submitted() {
        use crate::offer::lifecycle::{apply_cancel_submitted_rows, ReconcilePersistOptions};

        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        let cancel_tx = "cd".repeat(32);
        store
            .upsert_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
            .expect("cancel_submitted");
        let (plan, cancel_rows) = classify_and_heal_local(&store, "m1").expect("plan");
        assert!(plan.authoritative.is_empty());
        assert!(plan.heal_only.is_empty());
        assert_eq!(cancel_rows.len(), 1);
        apply_cancel_submitted_rows(
            &store,
            &cancel_rows,
            &ReconcilePersistOptions {
                action: "cancel_submitted_orphan_reconcile",
                venue: None,
                dexie_error: None,
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "cancel_submitted");
    }
}
