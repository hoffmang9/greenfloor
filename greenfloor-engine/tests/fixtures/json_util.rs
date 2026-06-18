use serde_json::{json, Value};

pub fn parse_json_output(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout).trim().to_string();
    if text.is_empty() {
        return json!({});
    }
    if let Some(start) = text.find('{') {
        return serde_json::from_str(&text[start..]).expect("parse json stdout");
    }
    serde_json::from_str(&text).expect("parse json stdout")
}
