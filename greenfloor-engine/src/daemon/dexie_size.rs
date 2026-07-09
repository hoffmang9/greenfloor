//! Dexie list helpers used by daemon reconcile (offer size / status indexing).

use std::collections::HashMap;

use serde_json::Value;

use crate::hex::normalize_hex_id;
use crate::offer::dexie_payload::dexie_offer_status;

/// Lookup keys for matching a Dexie offer payload to local `offer_state.offer_id`.
///
/// Prefers normalized 64-hex `trade_id` (canonical Coinset / Dexie trade id), then
/// Dexie bech32 `id`.
#[must_use]
pub fn dexie_offer_lookup_keys(offer_obj: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(trade_id) = offer_obj
        .get("trade_id")
        .and_then(Value::as_str)
        .map(normalize_hex_id)
        .filter(|value| value.len() == 64)
    {
        if seen.insert(trade_id.clone()) {
            keys.push(trade_id);
        }
    }
    if let Some(id) = offer_obj
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        if seen.insert(id.clone()) {
            keys.push(id);
        }
    }
    keys
}

/// Index Dexie list/augment payloads by every lookup key (`trade_id` ∪ bech32 `id`).
///
/// Used by cancel targeting and any path that must resolve local `offer_id` to Dexie
/// status without re-walking JSON.
#[must_use]
pub fn dexie_status_index(offers: &[Value]) -> HashMap<String, i64> {
    let mut by_key = HashMap::new();
    for offer in offers {
        let Some(obj) = offer.as_object() else {
            continue;
        };
        let Some(status) = dexie_offer_status(offer) else {
            continue;
        };
        for key in dexie_offer_lookup_keys(obj) {
            by_key.insert(key, status);
        }
    }
    by_key
}

/// Build offered-base size by offer id for strategy / pending-visibility lookups.
///
/// Indexes both normalized Dexie `trade_id` (64-hex) and Dexie bech32 `id` so lookups
/// succeed whether local `offer_state.offer_id` is the canonical trade id or the
/// venue list id.
pub fn build_dexie_size_by_offer_id(offers: &[Value], base_asset_id: &str) -> HashMap<String, i64> {
    let clean_base = base_asset_id.trim().to_ascii_lowercase();
    let mut result = HashMap::default();
    for offer in offers {
        let Some(offer_obj) = offer.as_object() else {
            continue;
        };
        let keys = dexie_offer_lookup_keys(offer_obj);
        if keys.is_empty() {
            continue;
        }
        let Some(offered) = offer_obj.get("offered").and_then(Value::as_array) else {
            continue;
        };
        for item in offered {
            let Some(item_obj) = item.as_object() else {
                continue;
            };
            let asset_id = item_obj
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if asset_id != clean_base {
                continue;
            }
            if let Some(size) = item_obj.get("amount").and_then(Value::as_i64) {
                if size > 0 {
                    for key in &keys {
                        result.insert(key.clone(), size);
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_dexie_size_map_indexes_trade_id_and_bech32_id() {
        let trade_id = "ab".repeat(32);
        let bech32_id = "7hj4tAYZEm9xTTniZiEVsPZ3mAnWvdposXizL3kDcjvo";
        let offers = vec![json!({
            "id": bech32_id,
            "trade_id": format!("0x{trade_id}"),
            "offered": [{"id": "base", "amount": 5}]
        })];
        let sizes = build_dexie_size_by_offer_id(&offers, "base");
        assert_eq!(sizes.get(&trade_id).copied(), Some(5));
        assert_eq!(sizes.get(bech32_id).copied(), Some(5));
    }

    #[test]
    fn builds_dexie_size_map_from_id_only_when_trade_id_absent() {
        let offers = vec![json!({
            "id": "offer-1",
            "offered": [{"id": "asset1", "amount": 5}]
        })];
        let sizes = build_dexie_size_by_offer_id(&offers, "asset1");
        assert_eq!(sizes.get("offer-1").copied(), Some(5));
    }

    #[test]
    fn dexie_status_index_keys_trade_id_and_bech32_id() {
        let trade_id = "ab".repeat(32);
        let bech32_id = "7hj4tAYZEm9xTTniZiEVsPZ3mAnWvdposXizL3kDcjvo";
        let offers = vec![json!({
            "id": bech32_id,
            "trade_id": format!("0x{trade_id}"),
            "status": 0,
        })];
        let index = dexie_status_index(&offers);
        assert_eq!(index.get(&trade_id).copied(), Some(0));
        assert_eq!(index.get(bech32_id).copied(), Some(0));
    }
}
