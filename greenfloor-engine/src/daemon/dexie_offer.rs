//! Typed accessors for Dexie offer JSON payloads used across reconcile and coin-ops.

use serde_json::Value;

/// Dexie offer body (list entry or single-offer lookup), kept as JSON for venue fidelity.
#[derive(Debug, Clone)]
pub struct DexieOfferPayload(pub Value);

impl DexieOfferPayload {
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }

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

    pub fn status(&self) -> Option<i64> {
        crate::daemon::coinset_tx::dexie_offer_status(self.body())
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
}
