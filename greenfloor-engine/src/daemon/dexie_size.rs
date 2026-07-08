//! Dexie list helpers used by daemon reconcile (offer size bucketing).

use std::collections::HashMap;

use serde_json::Value;

use crate::hex::normalize_hex_id;

pub fn build_dexie_size_by_offer_id(offers: &[Value], base_asset_id: &str) -> HashMap<String, i64> {
    let clean_base = base_asset_id.trim().to_ascii_lowercase();
    let mut result = HashMap::default();
    for offer in offers {
        let Some(offer_obj) = offer.as_object() else {
            continue;
        };
        let offer_id = offer_obj
            .get("trade_id")
            .and_then(Value::as_str)
            .map(normalize_hex_id)
            .filter(|value| value.len() == 64)
            .unwrap_or_default();
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
    fn builds_dexie_size_map_prefers_trade_id() {
        let trade_id = "ab".repeat(32);
        let offers = vec![json!({
            "id": "7hj4tAYZEm9xTTniZiEVsPZ3mAnWvdposXizL3kDcjvo",
            "trade_id": format!("0x{trade_id}"),
            "offered": [{"id": "base", "amount": 5}]
        })];
        let sizes = build_dexie_size_by_offer_id(&offers, "base");
        assert_eq!(sizes.get(&trade_id).copied(), Some(5));
    }
}
