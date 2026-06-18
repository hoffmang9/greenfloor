use serde_json::Value;

use crate::offer::dexie_payload::extract_coinset_tx_ids_from_offer_payload;

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

pub fn build_dexie_size_by_offer_id(
    offers: &[Value],
    base_asset_id: &str,
) -> std::collections::HashMap<String, i64> {
    let clean_base = base_asset_id.trim().to_ascii_lowercase();
    let mut result = std::collections::HashMap::default();
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
    use crate::offer::dexie_payload::{
        extract_coin_ids_from_offer_payload, extract_coinset_tx_ids_from_offer_payload,
    };
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
        assert_eq!(extract_coin_ids_from_offer_payload(&payload), vec![coin]);
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
