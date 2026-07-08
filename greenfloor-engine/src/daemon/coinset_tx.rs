//! Classify Coinset websocket payloads into typed transaction / offer events.

use std::collections::HashSet;

use serde_json::Value;

use crate::hex::normalize_hex_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsOfferEvent {
    pub offer_id: String,
    pub status: String,
    pub tx_id: Option<String>,
    pub p2s: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsTransactionEvent {
    pub status: String,
    pub tx_ids: Vec<String>,
    pub p2s: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsEvent {
    Transaction(WsTransactionEvent),
    Offer(WsOfferEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassifiedWsPayload {
    pub mempool_tx_ids: Vec<String>,
    pub confirmed_tx_ids: Vec<String>,
    pub offer_events: Vec<WsOfferEvent>,
    pub observed_p2s: Vec<String>,
}

/// Coinset puzzle-hash field names on transaction/offer data objects.
const P2_KEYS: &[&str] = &["p2s", "incoming_p2s", "outgoing_p2s", "maker_p2s", "p2"];

fn push_hex64(raw: &str, out: &mut Vec<String>, seen: &mut HashSet<String>) {
    let normalized = normalize_hex_id(raw);
    if normalized.len() == 64 && seen.insert(normalized.clone()) {
        out.push(normalized);
    }
}

fn collect_p2s(value: &Value, out: &mut Vec<String>, seen: &mut HashSet<String>) {
    match value {
        Value::String(raw) => push_hex64(raw, out, seen),
        Value::Array(items) => {
            for item in items {
                collect_p2s(item, out, seen);
            }
        }
        _ => {}
    }
}

fn p2s_from_object(obj: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut p2s = Vec::new();
    let mut seen = HashSet::new();
    for key in P2_KEYS {
        if let Some(value) = obj.get(*key) {
            collect_p2s(value, &mut p2s, &mut seen);
        }
    }
    p2s
}

fn tx_ids_from_data(data: &Value) -> Vec<String> {
    let mut tx_ids = Vec::new();
    let mut seen = HashSet::new();
    if let Some(Value::Array(items)) = data.get("ids") {
        for item in items {
            if let Some(raw) = item.as_str() {
                push_hex64(raw, &mut tx_ids, &mut seen);
            }
        }
    }
    tx_ids
}

fn parse_transaction(data: &Value) -> WsTransactionEvent {
    let status = data
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let p2s = data.as_object().map(p2s_from_object).unwrap_or_default();
    WsTransactionEvent {
        status,
        tx_ids: tx_ids_from_data(data),
        p2s,
    }
}

fn parse_offer(data: &Value) -> Option<WsOfferEvent> {
    let offer_id_raw = data.get("offer_id").and_then(Value::as_str)?;
    let offer_id = normalize_hex_id(offer_id_raw);
    if offer_id.len() != 64 {
        return None;
    }
    let status = data
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if status.is_empty() {
        return None;
    }
    let tx_id = data
        .get("tx_id")
        .and_then(Value::as_str)
        .map(normalize_hex_id)
        .filter(|value| value.len() == 64);
    let p2s = data.as_object().map(p2s_from_object).unwrap_or_default();
    Some(WsOfferEvent {
        offer_id,
        status,
        tx_id,
        p2s,
    })
}

/// Parse a Coinset WS JSON payload into a typed event (`WsEnvelope` shape).
#[must_use]
pub fn parse_ws_event(payload: &Value) -> Option<WsEvent> {
    let message = payload.get("message").and_then(Value::as_object)?;
    let msg_type = message
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let data = message.get("data").cloned().unwrap_or(Value::Null);
    match msg_type.as_str() {
        "transaction" => Some(WsEvent::Transaction(parse_transaction(&data))),
        "offer" => parse_offer(&data).map(WsEvent::Offer),
        _ => None,
    }
}

/// Classify a Coinset WS JSON payload. Only `transaction` messages fill tx buckets;
/// `offer` messages produce offer events (+ observed p2s) only.
#[must_use]
pub fn classify_ws_payload(payload: &Value) -> ClassifiedWsPayload {
    let mut out = ClassifiedWsPayload::default();
    let mut p2_seen = HashSet::new();
    let Some(event) = parse_ws_event(payload) else {
        return out;
    };
    match event {
        WsEvent::Transaction(tx) => {
            for p2 in &tx.p2s {
                if p2_seen.insert(p2.clone()) {
                    out.observed_p2s.push(p2.clone());
                }
            }
            match tx.status.as_str() {
                "confirmed" => out.confirmed_tx_ids = tx.tx_ids,
                "pending" => out.mempool_tx_ids = tx.tx_ids,
                _ => {}
            }
        }
        WsEvent::Offer(offer) => {
            for p2 in &offer.p2s {
                if p2_seen.insert(p2.clone()) {
                    out.observed_p2s.push(p2.clone());
                }
            }
            out.offer_events.push(offer);
        }
    }
    out
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

    #[test]
    fn classify_envelope_transaction_pending_and_confirmed() {
        let tx = "ab".repeat(32);
        let pending = json!({
            "message": {
                "type": "transaction",
                "data": {"status": "pending", "ids": [tx], "p2s": ["cd".repeat(32)]}
            }
        });
        let classified = classify_ws_payload(&pending);
        assert_eq!(classified.mempool_tx_ids, vec![tx.clone()]);
        assert_eq!(classified.observed_p2s, vec!["cd".repeat(32)]);

        let confirmed = json!({
            "message": {
                "type": "transaction",
                "data": {"status": "confirmed", "ids": [tx]}
            }
        });
        let classified = classify_ws_payload(&confirmed);
        assert_eq!(classified.confirmed_tx_ids, vec![tx]);
    }

    #[test]
    fn classify_envelope_offer_event_does_not_seed_tx_buckets() {
        let offer_id = "ab".repeat(32);
        let tx = "cd".repeat(32);
        let p2 = "ef".repeat(32);
        let payload = json!({
            "message": {
                "type": "offer",
                "data": {
                    "offer_id": format!("0x{offer_id}"),
                    "status": "pending",
                    "tx_id": tx,
                    "p2s": [p2],
                }
            }
        });
        let classified = classify_ws_payload(&payload);
        assert_eq!(classified.offer_events.len(), 1);
        assert_eq!(classified.offer_events[0].offer_id, offer_id);
        assert_eq!(classified.offer_events[0].status, "pending");
        assert_eq!(
            classified.offer_events[0].tx_id.as_deref(),
            Some(tx.as_str())
        );
        assert!(classified.mempool_tx_ids.is_empty());
        assert!(classified.confirmed_tx_ids.is_empty());
        assert_eq!(classified.observed_p2s, vec![p2]);
    }

    #[test]
    fn non_envelope_payload_is_ignored() {
        let tx_id = "c".repeat(64);
        let classified = classify_ws_payload(&json!({"event": "mempool_seen", "tx_id": tx_id}));
        assert!(classified.mempool_tx_ids.is_empty());
        assert!(classified.confirmed_tx_ids.is_empty());
    }
}
