use std::path::Path;

use crate::error::{SignerError, SignerResult};

use super::file::ScanCheckpointFile;
use super::runtime::LoadCheckpointResult;

/// Load scan checkpoint.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_scan_checkpoint(
    checkpoint_file: &Path,
    network: &str,
    launcher_id: &str,
    include_spent: bool,
) -> SignerResult<LoadCheckpointResult> {
    if !checkpoint_file.exists() {
        return Ok(LoadCheckpointResult::empty());
    }
    let raw = std::fs::read_to_string(checkpoint_file).map_err(|err| {
        SignerError::Other(format!(
            "read checkpoint {}: {err}",
            checkpoint_file.display()
        ))
    })?;
    let parsed: ScanCheckpointFile = serde_json::from_str(&raw).map_err(|err| {
        SignerError::Other(format!(
            "parse checkpoint json {}: {err}",
            checkpoint_file.display()
        ))
    })?;
    if let Err(reason) = parsed.validate_params(network, launcher_id, include_spent) {
        return Ok(LoadCheckpointResult::Discarded(reason));
    }
    let (checkpoint, start_nonce) = parsed.into_loaded();
    Ok(LoadCheckpointResult::Loaded {
        checkpoint: Box::new(checkpoint),
        start_nonce,
    })
}
