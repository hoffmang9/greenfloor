use std::path::Path;

use crate::error::{SignerError, SignerResult};

use super::file::{CheckpointWriteMetadata, ScanCheckpointFile};
use super::runtime::LoadedCheckpoint;

/// Save scan checkpoint.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn save_scan_checkpoint(
    checkpoint_file: &Path,
    metadata: &CheckpointWriteMetadata<'_>,
    checkpoint: &LoadedCheckpoint,
) -> SignerResult<()> {
    if let Some(parent) = checkpoint_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| SignerError::Other(format!("create checkpoint dir: {err}")))?;
    }
    let payload = ScanCheckpointFile::from_loaded(checkpoint, metadata);
    std::fs::write(
        checkpoint_file,
        serde_json::to_string_pretty(&payload)
            .map_err(|err| SignerError::Other(format!("encode checkpoint json: {err}")))?,
    )
    .map_err(|err| SignerError::Other(format!("write checkpoint: {err}")))?;
    Ok(())
}
