//! Typed accessors and field extraction for Dexie offer JSON payloads.

use serde_json::Value;

const COINSET_TX_ID_KEYS: &[&str] = &[
    "tx_id",
    "txId",
    "transaction_id",
    "transactionId",
    "spend_bundle_name",
    "spendBundleName",
];

const COINSET_COIN_ID_KEYS: &[&str] = &[
    "id",
    "coin_id",
    "coinId",
    "name",
    "coin_name",
    "coinName",
    "involved_coins",
    "involvedCoins",
    "input_coins",
    "inputCoins",
    "output_coins",
    "outputCoins",
    "spent_coins",
    "spentCoins",
    "additions",
    "removals",
];

fn normalize_hex_hash(value: &str) -> String {
    value.trim().trim_start_matches("0x").to_ascii_lowercase()
}

fn looks_like_tx_id(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn add_candidate(candidate: &Value, tx_ids: &mut Vec<String>) {
    match candidate {
        Value::String(raw) => {
            let normalized = normalize_hex_hash(raw);
            if looks_like_tx_id(&normalized)
                && !tx_ids.iter().any(|existing| existing == &normalized)
            {
                tx_ids.push(normalized);
            }
        }
        Value::Array(items) => {
            for item in items {
                add_candidate(item, tx_ids);
            }
        }
        _ => {}
    }
}

fn walk_tx_id_node(node: &Value, tx_ids: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if COINSET_TX_ID_KEYS.iter().any(|candidate| candidate == key) {
                    add_candidate(value, tx_ids);
                }
                if matches!(value, Value::Object(_) | Value::Array(_)) {
                    walk_tx_id_node(value, tx_ids);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if matches!(item, Value::Object(_) | Value::Array(_)) {
                    walk_tx_id_node(item, tx_ids);
                }
            }
        }
        _ => {}
    }
}

fn add_coin_id_candidate(candidate: &Value, coin_ids: &mut Vec<String>) {
    match candidate {
        Value::String(raw) => {
            let normalized = normalize_hex_hash(raw);
            if looks_like_tx_id(&normalized)
                && !coin_ids.iter().any(|existing| existing == &normalized)
            {
                coin_ids.push(normalized);
            }
        }
        Value::Array(items) => {
            for item in items {
                add_coin_id_candidate(item, coin_ids);
            }
        }
        Value::Object(map) => {
            for key in COINSET_COIN_ID_KEYS {
                if let Some(value) = map.get(*key) {
                    add_coin_id_candidate(value, coin_ids);
                }
            }
        }
        _ => {}
    }
}

fn walk_coin_id_node(node: &Value, coin_ids: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if COINSET_COIN_ID_KEYS
                    .iter()
                    .any(|candidate| candidate == key)
                {
                    add_coin_id_candidate(value, coin_ids);
                }
                if matches!(value, Value::Object(_) | Value::Array(_)) {
                    walk_coin_id_node(value, coin_ids);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if matches!(item, Value::Object(_) | Value::Array(_)) {
                    walk_coin_id_node(item, coin_ids);
                }
            }
        }
        _ => {}
    }
}

#[must_use]
pub fn extract_coin_ids_from_offer_payload(payload: &Value) -> Vec<String> {
    let mut coin_ids = Vec::new();
    walk_coin_id_node(payload, &mut coin_ids);
    coin_ids
}

#[must_use]
pub fn extract_coinset_tx_ids_from_offer_payload(payload: &Value) -> Vec<String> {
    let mut tx_ids = Vec::new();
    walk_tx_id_node(payload, &mut tx_ids);
    tx_ids
}

pub fn dexie_offer_status(payload: &Value) -> Option<i64> {
    if let Some(status) = payload.get("status").and_then(Value::as_i64) {
        return Some(status);
    }
    payload
        .get("offer")
        .and_then(|offer| offer.get("status"))
        .and_then(Value::as_i64)
}

/// Dexie offer body (list entry or single-offer lookup), kept as JSON for venue fidelity.
#[derive(Debug, Clone)]
pub struct DexieOfferPayload(pub Value);

impl DexieOfferPayload {
    #[must_use]
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        self.0
    }

    /// Normalized offer object (unwraps nested `"offer"` when present).
    pub fn body(&self) -> &Value {
        if self.0.get("offer").and_then(Value::as_object).is_some() {
            self.0.get("offer").unwrap_or(&self.0)
        } else {
            &self.0
        }
    }

    pub fn id(&self) -> Option<String> {
        let id = self
            .body()
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if id.is_empty() {
            None
        } else {
            Some(id.to_string())
        }
    }

    #[must_use]
    pub fn status(&self) -> Option<i64> {
        dexie_offer_status(self.body())
    }
}

impl From<Value> for DexieOfferPayload {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}

impl From<DexieOfferPayload> for Value {
    fn from(value: DexieOfferPayload) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn id_and_status_read_nested_offer_body() {
        let offer = DexieOfferPayload::new(json!({
            "offer": {"id": "offer-1", "status": 4}
        }));
        assert_eq!(offer.id().as_deref(), Some("offer-1"));
        assert_eq!(offer.status(), Some(4));
    }

    #[test]
    fn extracts_nested_tx_id() {
        let payload = json!({"offer": {"tx_id": "a".repeat(64)}});
        assert_eq!(
            extract_coinset_tx_ids_from_offer_payload(&payload),
            vec!["a".repeat(64)]
        );
    }

    #[test]
    fn extracts_coin_id_from_nested_payload() {
        let coin = "b".repeat(64);
        let payload = json!({"offer": {"coin_id": coin}});
        assert_eq!(extract_coin_ids_from_offer_payload(&payload), vec![coin]);
    }
}
