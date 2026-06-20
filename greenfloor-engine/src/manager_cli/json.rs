//! JSON output for the native manager CLI (`--json`).

use serde::Serialize;
use serde_json::Value;

use crate::cli_util;
use crate::error::SignerResult;

/// Explicit JSON output mode for manager commands (no process-global state).
#[derive(Debug, Clone)]
pub struct ManagerOutput {
    compact: bool,
}

impl ManagerOutput {
    pub fn new(compact: bool) -> Self {
        Self { compact }
    }

    pub fn emit_json(&self, value: &Value) -> SignerResult<()> {
        cli_util::print_json_value(value, self.compact)
    }

    pub fn emit_serialized<T: Serialize>(&self, value: &T) -> SignerResult<()> {
        cli_util::print_json(value, self.compact)
    }
}
