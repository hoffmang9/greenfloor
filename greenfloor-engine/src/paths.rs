//! Canonical path helpers shared across config, manager CLI, and daemon.

use std::path::{Path, PathBuf};

pub fn expand_home(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let raw = path.to_string_lossy();
    if raw == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(stripped) = raw.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_home_resolves_tilde_prefix() {
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(
                expand_home(Path::new("~/.greenfloor")),
                PathBuf::from(home).join(".greenfloor")
            );
        }
    }
}
