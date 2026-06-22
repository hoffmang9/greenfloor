//! Scan checkpoint persistence: resume state for vault coinset scans.

mod file;
mod load;
mod runtime;
mod save;

pub use file::CheckpointWriteMetadata;
pub use load::load_scan_checkpoint;
pub use runtime::{
    LoadCheckpointDiscardReason, LoadCheckpointResult, LoadedCheckpoint, ParentLineageEntry,
};
pub use save::save_scan_checkpoint;

#[cfg(test)]
mod tests;
