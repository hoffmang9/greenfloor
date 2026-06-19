use serde_json::{json, Value};

pub fn parse_json_output(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout).trim().to_string();
    if text.is_empty() {
        return json!({});
    }
    serde_json::from_str(&text).unwrap_or_else(|err| {
        panic!("stdout must be exactly one JSON document: {err}; got: {text:?}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_output_accepts_single_document() {
        let value = parse_json_output(br#"{"ok":true}"#);
        assert_eq!(value.get("ok"), Some(&json!(true)));
    }

    #[test]
    #[should_panic(expected = "stdout must be exactly one JSON document")]
    fn parse_json_output_rejects_leading_noise() {
        let _ = parse_json_output(b"log line\n{\"ok\":true}");
    }
}
