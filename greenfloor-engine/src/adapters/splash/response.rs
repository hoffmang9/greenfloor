use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashResponse {
    body: Value,
}

impl SplashResponse {
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
    use super::SplashResponse;
    use serde_json::json;

    #[test]
    fn splash_response_reads_success_id_and_error() {
        let response = SplashResponse::from_value(json!({
            "success": true,
            "id": "trade-1",
            "error": "",
        }));
        assert!(response.success());
        assert_eq!(response.offer_id(), Some("trade-1"));
        assert_eq!(response.error_text(), "");
    }
}
