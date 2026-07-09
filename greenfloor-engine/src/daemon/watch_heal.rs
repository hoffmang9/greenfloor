//! Heal durable offer watches from Dexie list payloads for pre-upgrade rows.

use std::collections::HashSet;

use serde_json::Value;

use crate::error::SignerResult;
use crate::offer::dexie_payload::extract_coin_ids_from_offer_payload;
use crate::storage::SqliteStore;

use super::dexie_size::dexie_offer_lookup_keys;

/// Insert missing coin watches for Dexie-authoritative offers that have none yet.
///
/// Coin-ops then reads only `offer_coin_watches` (no Dexie scrape at execution time).
///
/// # Errors
///
/// Returns an error if `SQLite` reads/writes fail.
pub fn heal_missing_watches_from_dexie_offers(
    store: &SqliteStore,
    market_id: &str,
    dexie_offer_ids: &HashSet<String>,
    offers: &[Value],
) -> SignerResult<()> {
    if dexie_offer_ids.is_empty() || offers.is_empty() {
        return Ok(());
    }
    for raw in offers {
        let Some(obj) = raw.as_object() else {
            continue;
        };
        let lookup_keys = dexie_offer_lookup_keys(obj);
        let Some(local_offer_id) = lookup_keys
            .iter()
            .find(|key| dexie_offer_ids.contains(key.as_str()))
            .cloned()
        else {
            continue;
        };
        if store.offer_has_coin_watches(&local_offer_id)? {
            continue;
        }
        let coin_ids = extract_coin_ids_from_offer_payload(raw);
        if coin_ids.is_empty() {
            continue;
        }
        store.ensure_offer_coin_watches(&local_offer_id, market_id, &coin_ids, &[])?;
    }
    Ok(())
}
