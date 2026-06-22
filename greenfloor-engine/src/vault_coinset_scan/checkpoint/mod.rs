mod load;
mod save;
mod types;

pub use load::load_scan_checkpoint;
pub use save::save_scan_checkpoint;
pub use types::{LoadedCheckpoint, ParentLineageEntry, SaveCheckpointParams};

use std::collections::BTreeMap;
use std::path::Path;

use crate::paths::expand_home;

#[must_use]
pub fn clear_cache_files(paths: &[String]) -> BTreeMap<String, String> {
    let mut results = BTreeMap::new();
    for raw_path in paths {
        let clean = raw_path.trim();
        if clean.is_empty() {
            continue;
        }
        let path = expand_home(Path::new(clean));
        let key = path.display().to_string();
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    results.insert(key, "deleted".to_string());
                }
                Err(err) => {
                    results.insert(key, format!("delete_failed:{err}"));
                }
            }
        } else {
            results.insert(key, "not_found".to_string());
        }
    }
    results
}

#[cfg(test)]
mod tests;
