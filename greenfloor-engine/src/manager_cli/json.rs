//! JSON output for the native manager CLI (`--json`).

#[cfg(test)]
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

use crate::cli_util;
use crate::error::SignerResult;

/// Explicit JSON output mode for manager commands (no process-global state).
#[derive(Debug, Clone)]
pub struct ManagerOutput {
    compact: bool,
    #[cfg(test)]
    capture_buffer: Option<Arc<Mutex<Vec<Value>>>>,
}

impl ManagerOutput {
    pub fn new(compact: bool) -> Self {
        Self {
            compact,
            #[cfg(test)]
            capture_buffer: None,
        }
    }

    #[cfg(test)]
    pub fn capturing(compact: bool) -> (Self, Arc<Mutex<Vec<Value>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                compact,
                capture_buffer: Some(buffer.clone()),
            },
            buffer,
        )
    }

    pub fn emit_json(&self, value: &Value) -> SignerResult<()> {
        #[cfg(test)]
        if let Some(buffer) = &self.capture_buffer {
            if let Ok(mut entries) = buffer.lock() {
                entries.push(value.clone());
            }
        }
        cli_util::print_json_value(value, self.compact)
    }

    pub fn emit_serialized<T: Serialize>(&self, value: &T) -> SignerResult<()> {
        #[cfg(test)]
        if let Some(buffer) = &self.capture_buffer {
            if let Ok(json_value) = serde_json::to_value(value) {
                if let Ok(mut entries) = buffer.lock() {
                    entries.push(json_value);
                }
            }
        }
        cli_util::print_json(value, self.compact)
    }
}
