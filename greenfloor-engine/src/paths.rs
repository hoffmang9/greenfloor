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

/// Walk upward from `start` until a directory contains `config/cats.yaml`.
pub fn find_repo_root_from(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if current.join("config/cats.yaml").is_file() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Resolve the GreenFloor repository root for cwd-independent config lookup.
pub fn resolve_repo_root() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("GREENFLOOR_REPO_ROOT") {
        let path = PathBuf::from(raw.trim());
        if path.join("config/cats.yaml").is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent().and_then(find_repo_root_from) {
            return Some(root);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = find_repo_root_from(&cwd) {
            return Some(root);
        }
    }
    None
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

    #[test]
    fn find_repo_root_from_engine_directory() {
        let engine_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = engine_dir.parent().expect("repo root");
        assert_eq!(
            find_repo_root_from(&engine_dir),
            Some(repo_root.to_path_buf())
        );
    }
}
