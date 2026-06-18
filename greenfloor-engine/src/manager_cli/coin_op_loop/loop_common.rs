use crate::error::{SignerError, SignerResult};

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
