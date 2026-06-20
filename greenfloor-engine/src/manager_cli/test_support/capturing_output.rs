//! Test-only JSON capture sidecar for manager CLI in-process tests.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct TestJsonCapture {
    buffer: Arc<Mutex<Vec<Value>>>,
}

impl TestJsonCapture {
    pub fn new() -> (Self, Arc<Mutex<Vec<Value>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                buffer: buffer.clone(),
            },
            buffer,
        )
    }

    pub fn record_json(&self, value: &Value) {
        if let Ok(mut entries) = self.buffer.lock() {
            entries.push(value.clone());
        }
    }

    pub fn record_serialized<T: Serialize>(&self, value: &T) {
        if let Ok(json_value) = serde_json::to_value(value) {
            self.record_json(&json_value);
        }
    }
}
