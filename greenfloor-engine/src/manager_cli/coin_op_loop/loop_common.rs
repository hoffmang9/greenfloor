use serde_json::{json, Value};

use crate::error::{SignerError, SignerResult};

use crate::manager_cli::context::ManagerContext;

use super::until_ready::{until_ready_exit_code, UntilReadyCompletion, UntilReadyWaitMode};

pub(super) fn validate_until_ready_mode(
    wait: UntilReadyWaitMode,
    size_base_units: Option<i64>,
) -> SignerResult<()> {
    if wait.until_ready && wait.no_wait {
        return Err(SignerError::Other(
            "until-ready mode requires wait mode (do not pass --no-wait)".to_string(),
        ));
    }
    if wait.until_ready && size_base_units.as_ref().is_none_or(|value| *value <= 0) {
        return Err(SignerError::Other(
            "until-ready mode requires --size-base-units".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn finish_coin_op_command(
    mgr: &ManagerContext,
    wait: UntilReadyWaitMode,
    completion: UntilReadyCompletion,
    success_payload: Value,
) -> SignerResult<i32> {
    match completion {
        UntilReadyCompletion::Exit { code, payload } => {
            if let Some(payload) = payload {
                mgr.emit_json(&payload)?;
            }
            Ok(code)
        }
        UntilReadyCompletion::Completed { stop_reason } => {
            let mut payload = success_payload;
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("stop_reason".to_string(), json!(stop_reason));
            }
            mgr.emit_json(&payload)?;
            Ok(until_ready_exit_code(wait.until_ready, &stop_reason))
        }
    }
}
