use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

use super::until_ready::{until_ready_exit_code, UntilReadyCompletion};
use crate::manager_cli::json::ManagerOutput;

pub(super) fn validate_until_ready_mode(
    until_ready: bool,
    no_wait: bool,
    size_base_units: Option<i64>,
) -> SignerResult<()> {
    if until_ready && no_wait {
        return Err(SignerError::Other(
            "until-ready mode requires wait mode (do not pass --no-wait)".to_string(),
        ));
    }
    if until_ready && size_base_units.filter(|value| *value > 0).is_none() {
        return Err(SignerError::Other(
            "until-ready mode requires --size-base-units".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn finish_coin_op_command(
    output: &ManagerOutput,
    until_ready: bool,
    completion: UntilReadyCompletion,
    success_payload: Value,
) -> SignerResult<i32> {
    match completion {
        UntilReadyCompletion::Exit { code, payload } => {
            if let Some(payload) = payload {
                output.emit_json(&payload)?;
            }
            Ok(code)
        }
        UntilReadyCompletion::Completed { stop_reason } => {
            let mut payload = success_payload;
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("stop_reason".to_string(), json!(stop_reason));
            }
            output.emit_json(&payload)?;
            Ok(until_ready_exit_code(until_ready, &stop_reason))
        }
    }
}
