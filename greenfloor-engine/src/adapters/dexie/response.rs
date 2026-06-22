use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexieResponse {
    body: Value,
}

impl DexieResponse {
    #[must_use]
    pub fn from_value(body: Value) -> Self {
        Self { body }
    }

    #[must_use]
    pub fn into_value(self) -> Value {
        self.body
    }

    #[must_use]
    pub fn body(&self) -> &Value {
        &self.body
    }

    #[must_use]
    pub fn success(&self) -> bool {
        self.body.get("success").and_then(Value::as_bool) == Some(true)
    }

    /// True when the payload explicitly sets `"success": false`.
    ///
    /// Dexie `get_offer` responses often omit `success` entirely when the offer
    /// is present; callers must not treat a missing field as failure.
    #[must_use]
    pub fn is_explicit_failure(&self) -> bool {
        self.body.get("success").and_then(Value::as_bool) == Some(false)
    }

    #[must_use]
    pub fn offer_payload(&self) -> Option<&Value> {
        self.body.get("offer")
    }

    #[must_use]
    pub fn offer_id(&self) -> Option<&str> {
        self.body
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    #[must_use]
    pub fn error_text(&self) -> &str {
        self.body
            .get("error")
            .and_then(Value::as_str)
            .map_or("", str::trim)
    }
}

#[cfg(test)]
mod tests {
    use super::DexieResponse;
    use serde_json::{json, Value};

    #[test]
    fn dexie_response_reads_success_id_and_error() {
        let response = DexieResponse::from_value(json!({
            "success": true,
            "id": "offer-1",
            "error": "",
        }));
        assert!(response.success());
        assert_eq!(response.offer_id(), Some("offer-1"));
        assert_eq!(response.error_text(), "");
    }

    #[test]
    fn dexie_response_is_explicit_failure_only_when_success_is_false() {
        let ok = DexieResponse::from_value(json!({"offer": {"id": "offer-1"}}));
        assert!(!ok.is_explicit_failure());
        assert!(!ok.success());

        let failed = DexieResponse::from_value(json!({"success": false, "error": "missing"}));
        assert!(failed.is_explicit_failure());
    }

    #[test]
    fn dexie_response_offer_payload_reads_nested_offer() {
        let response = DexieResponse::from_value(json!({
            "offer": {"id": "offer-1", "status": 6},
        }));
        assert_eq!(
            response
                .offer_payload()
                .and_then(|offer| offer.get("id"))
                .and_then(Value::as_str),
            Some("offer-1")
        );
    }
}
