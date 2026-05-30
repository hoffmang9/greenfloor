use serde_json::Value;

const COINSET_TX_ID_KEYS: &[&str] = &[
    "tx_id",
    "txId",
    "transaction_id",
    "transactionId",
    "spend_bundle_name",
    "spendBundleName",
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
            if looks_like_tx_id(&normalized) && !tx_ids.iter().any(|existing| existing == &normalized) {
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

fn walk_node(node: &Value, tx_ids: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if COINSET_TX_ID_KEYS.iter().any(|candidate| candidate == key) {
                    add_candidate(value, tx_ids);
                }
                if matches!(value, Value::Object(_) | Value::Array(_)) {
                    walk_node(value, tx_ids);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if matches!(item, Value::Object(_) | Value::Array(_)) {
                    walk_node(item, tx_ids);
                }
            }
        }
        _ => {}
    }
}

const COINSET_COIN_ID_KEYS: &[&str] = &[
    "id",
    "coin_id",
    "coinId",
    "name",
    "coin_name",
    "coinName",
];

fn add_coin_id_candidate(candidate: &Value, coin_ids: &mut Vec<String>) {
    match candidate {
        Value::String(raw) => {
            let normalized = normalize_hex_hash(raw);
            if looks_like_tx_id(&normalized) && !coin_ids.iter().any(|existing| existing == &normalized)
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
                if COINSET_COIN_ID_KEYS.iter().any(|candidate| candidate == key) {
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

pub fn extract_coin_ids_from_offer_payload(payload: &Value) -> Vec<String> {
    let mut coin_ids = Vec::new();
    walk_coin_id_node(payload, &mut coin_ids);
    coin_ids
}

pub fn extract_coinset_tx_ids_from_offer_payload(payload: &Value) -> Vec<String> {
    let mut tx_ids = Vec::new();
    walk_node(payload, &mut tx_ids);
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

pub fn classify_ws_payload_tx_ids(payload: &Value) -> (Vec<String>, Vec<String>) {
    let event_hint = payload
        .get("event")
        .or_else(|| payload.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let tx_ids = extract_coinset_tx_ids_from_offer_payload(payload);
    if tx_ids.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let is_confirmed = payload
        .get("confirmed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || payload
            .get("in_block")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || event_hint.contains("confirm")
        || event_hint.contains("block");
    if is_confirmed {
        return (Vec::new(), tx_ids);
    }
    (tx_ids, Vec::new())
}

pub fn build_dexie_size_by_offer_id(offers: &[Value], base_asset_id: &str) -> std::collections::HashMap<String, i64> {
    let clean_base = base_asset_id.trim().to_ascii_lowercase();
    let mut result = std::collections::HashMap::new();
    for offer in offers {
        let Some(offer_obj) = offer.as_object() else {
            continue;
        };
        let offer_id = offer_obj
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if offer_id.is_empty() {
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
                    result.insert(offer_id.clone(), size);
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
        assert_eq!(
            extract_coin_ids_from_offer_payload(&payload),
            vec![coin]
        );
    }

    #[test]
    fn builds_dexie_size_map() {
        let offers = vec![json!({
            "id": "offer-1",
            "offered": [{"id": "asset1", "amount": 5}]
        })];
        let sizes = build_dexie_size_by_offer_id(&offers, "asset1");
        assert_eq!(sizes.get("offer-1").copied(), Some(5));
    }
}
