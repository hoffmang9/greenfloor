//! JSON output mode for the native manager CLI (`--json`).

use serde_json::Value;

use crate::cli_util;
use crate::error::SignerResult;

static COMPACT_JSON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_json_output_compact(enabled: bool) {
    COMPACT_JSON.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub fn json_output_compact() -> bool {
    COMPACT_JSON.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn emit_json(value: &Value) -> SignerResult<()> {
    cli_util::print_json_value(value, json_output_compact())
}
