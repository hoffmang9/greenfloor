//! JSON output for the native manager CLI (`--json`).

use serde::Serialize;
use serde_json::Value;

use crate::cli_util;
use crate::error::SignerResult;

/// Explicit JSON output mode for manager commands (no process-global state).
#[derive(Debug, Clone)]
pub struct ManagerOutput {
    compact: bool,
    #[cfg(test)]
    capture: Option<std::sync::Arc<std::sync::Mutex<Vec<Value>>>>,
}

impl ManagerOutput {
    pub fn new(compact: bool) -> Self {
        Self {
            compact,
            #[cfg(test)]
            capture: None,
        }
    }

    #[cfg(test)]
    pub fn capturing(compact: bool) -> (Self, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        let capture = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Self {
                compact,
                capture: Some(capture.clone()),
            },
            capture,
        )
    }

    pub fn emit_json(&self, value: &Value) -> SignerResult<()> {
        #[cfg(test)]
        if let Some(capture) = &self.capture {
            if let Ok(mut entries) = capture.lock() {
                entries.push(value.clone());
            }
        }
        cli_util::print_json_value(value, self.compact)
    }

    pub fn emit_serialized<T: Serialize>(&self, value: &T) -> SignerResult<()> {
        #[cfg(test)]
        if let Some(capture) = &self.capture {
            if let Ok(json_value) = serde_json::to_value(value) {
                if let Ok(mut entries) = capture.lock() {
                    entries.push(json_value);
                }
            }
        }
        cli_util::print_json(value, self.compact)
    }
}
