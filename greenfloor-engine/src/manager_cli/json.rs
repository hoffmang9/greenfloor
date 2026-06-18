//! JSON output helpers for the native manager CLI.

use serde::Serialize;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};

static COMPACT_JSON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_json_output_compact(enabled: bool) {
    COMPACT_JSON.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub fn json_output_compact() -> bool {
    COMPACT_JSON.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn print_json(value: &impl Serialize) -> Result<(), String> {
    let text = if json_output_compact() {
        serde_json::to_string(value).map_err(|err| format!("json encode failed: {err}"))?
    } else {
        serde_json::to_string_pretty(value).map_err(|err| format!("json encode failed: {err}"))?
    };
    println!("{text}");
    Ok(())
}

pub fn print_json_value(value: &Value) -> Result<(), String> {
    let text = if json_output_compact() {
        serde_json::to_string(value).map_err(|err| format!("json encode failed: {err}"))?
    } else {
        serde_json::to_string_pretty(value).map_err(|err| format!("json encode failed: {err}"))?
    };
    println!("{text}");
    Ok(())
}

pub fn emit_json(value: &Value) -> SignerResult<()> {
    print_json_value(value).map_err(SignerError::Other)
}
