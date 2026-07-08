//! Parse Coinset websocket envelopes into typed transaction / offer events.

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_envelope_transaction_pending_and_confirmed() {
        let tx = "ab".repeat(32);
        let pending = json!({
            "message": {
                "type": "transaction",
                "data": {"status": "pending", "ids": [tx], "p2s": ["cd".repeat(32)]}
            }
        });
        match parse_ws_event(&pending).expect("event") {
            WsEvent::Transaction(event) => {
                assert_eq!(event.status, "pending");
                assert_eq!(event.tx_ids, vec![tx.clone()]);
                assert_eq!(event.p2s, vec!["cd".repeat(32)]);
            }
            WsEvent::Offer(_) => panic!("expected transaction"),
        }

        let confirmed = json!({
            "message": {
                "type": "transaction",
                "data": {"status": "confirmed", "ids": [tx]}
            }
        });
        match parse_ws_event(&confirmed).expect("event") {
            WsEvent::Transaction(event) => {
                assert_eq!(event.status, "confirmed");
                assert_eq!(event.tx_ids, vec![tx]);
            }
            WsEvent::Offer(_) => panic!("expected transaction"),
        }
    }

    #[test]
    fn parse_envelope_offer_event_keeps_tx_id_on_event_only() {
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
        match parse_ws_event(&payload).expect("event") {
            WsEvent::Offer(event) => {
                assert_eq!(event.offer_id, offer_id);
                assert_eq!(event.status, "pending");
                assert_eq!(event.tx_id.as_deref(), Some(tx.as_str()));
                assert_eq!(event.p2s, vec![p2]);
            }
            WsEvent::Transaction(_) => panic!("expected offer"),
        }
    }

    #[test]
    fn non_envelope_payload_is_ignored() {
        let tx_id = "c".repeat(64);
        assert!(parse_ws_event(&json!({"event": "mempool_seen", "tx_id": tx_id})).is_none());
    }
}
