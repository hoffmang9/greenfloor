use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

const BRIDGE_MODULE: &str = "greenfloor.daemon.bridge_subprocess";

/// Python IO surface invoked by the Rust daemon orchestrator.
pub trait DaemonPythonBridge: Send + Sync {
    fn call_method(&self, method: &str, kwargs: &Value) -> SignerResult<Value>;

    /// Subprocess bridge workers can run in parallel; in-process PyO3 bridge must stay sequential.
    fn supports_parallel_workers(&self) -> bool {
        false
    }
}

pub struct SubprocessPythonBridge {
    python: PathBuf,
}

impl SubprocessPythonBridge {
    pub fn discover() -> SignerResult<Self> {
        if let Ok(raw) = std::env::var("GREENFLOOR_PYTHON") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Ok(Self {
                    python: PathBuf::from(trimmed),
                });
            }
        }
        if let Ok(raw) = std::env::var("VIRTUAL_ENV") {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                let candidate = PathBuf::from(trimmed).join("bin").join("python");
                if candidate.is_file() {
                    return Ok(Self { python: candidate });
                }
            }
        }
        for candidate in ["python3", "python"] {
            if let Some(path) = which::which(candidate).ok() {
                return Ok(Self { python: path });
            }
        }
        Err(SignerError::Other(
            "python interpreter not found; set GREENFLOOR_PYTHON or activate a venv".to_string(),
        ))
    }
}

impl DaemonPythonBridge for SubprocessPythonBridge {
    fn call_method(&self, method: &str, kwargs: &Value) -> SignerResult<Value> {
        let request = json!({
            "method": method,
            "kwargs": kwargs,
        });
        let mut child = Command::new(&self.python)
            .arg("-m")
            .arg(BRIDGE_MODULE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                SignerError::Other(format!(
                    "failed to spawn python bridge {}: {err}",
                    self.python.display()
                ))
            })?;
        if let Some(stdin) = child.stdin.as_mut() {
            let payload = serde_json::to_vec(&request).map_err(|err| {
                SignerError::Other(format!("failed to encode python bridge request: {err}"))
            })?;
            stdin.write_all(&payload).map_err(|err| {
                SignerError::Other(format!("failed to write python bridge stdin: {err}"))
            })?;
        }
        let output = child.wait_with_output().map_err(|err| {
            SignerError::Other(format!("failed to wait on python bridge: {err}"))
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SignerError::Other(format!(
                "python bridge exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }
        decode_bridge_response(&output.stdout)
    }

    fn supports_parallel_workers(&self) -> bool {
        true
    }
}

pub fn decode_bridge_response(stdout: &[u8]) -> SignerResult<Value> {
    let response: Value = serde_json::from_slice(stdout).map_err(|err| {
        SignerError::Other(format!(
            "failed to decode python bridge stdout: {err}; raw={}",
            String::from_utf8_lossy(stdout)
        ))
    })?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("python bridge returned ok=false");
        return Err(SignerError::Other(message.to_string()));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| SignerError::Other("python bridge missing result payload".to_string()))
}

pub fn default_bridge() -> SignerResult<SubprocessPythonBridge> {
    SubprocessPythonBridge::discover()
}
