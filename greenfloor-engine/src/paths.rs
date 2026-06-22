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

const DEFAULT_TESTNET_MARKETS_CONFIG: &str = "~/.greenfloor/config/testnet-markets.yaml";

/// Default testnet markets overlay when `~/.greenfloor/config/testnet-markets.yaml` exists.
#[must_use]
pub fn default_testnet_markets_config_path() -> Option<PathBuf> {
    let home_default = expand_home(Path::new(DEFAULT_TESTNET_MARKETS_CONFIG));
    home_default.exists().then_some(home_default)
}

/// How operator entrypoints resolve `--testnet-markets-config` / `testnet_markets_path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestnetMarketsPathPolicy {
    /// Daemon CLI: empty → `None`; explicit path → `Some` only when the file exists.
    RequireExistingFile,
    /// Manager CLI: empty → [`default_testnet_markets_config_path`]; explicit path → always `Some`.
    CliWithDefault,
}

/// Resolve a testnet markets config path for the given entrypoint policy.
#[must_use]
pub fn resolve_testnet_markets_path(
    raw: &str,
    policy: TestnetMarketsPathPolicy,
) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return match policy {
            TestnetMarketsPathPolicy::RequireExistingFile => None,
            TestnetMarketsPathPolicy::CliWithDefault => default_testnet_markets_config_path(),
        };
    }

    let path = PathBuf::from(trimmed);
    match policy {
        TestnetMarketsPathPolicy::RequireExistingFile => path.exists().then_some(path),
        TestnetMarketsPathPolicy::CliWithDefault => Some(path),
    }
}

/// Walk upward from `start` until a directory contains `config/cats.yaml`.
#[must_use]
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

/// Resolve the `GreenFloor` repository root for cwd-independent config lookup.
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
    fn resolve_testnet_markets_path_cli_with_default_uses_explicit_without_exists_check() {
        let path = resolve_testnet_markets_path(
            "/tmp/nonexistent-testnet-markets.yaml",
            TestnetMarketsPathPolicy::CliWithDefault,
        )
        .expect("explicit cli path");
        assert_eq!(path, PathBuf::from("/tmp/nonexistent-testnet-markets.yaml"));
    }

    #[test]
    fn resolve_testnet_markets_path_require_existing_file_rejects_missing_explicit() {
        assert!(resolve_testnet_markets_path(
            "/tmp/nonexistent-testnet-markets.yaml",
            TestnetMarketsPathPolicy::RequireExistingFile,
        )
        .is_none());
    }

    #[test]
    fn resolve_testnet_markets_path_empty_input_follows_policy() {
        assert!(
            resolve_testnet_markets_path("", TestnetMarketsPathPolicy::RequireExistingFile)
                .is_none()
        );
        assert_eq!(
            resolve_testnet_markets_path("", TestnetMarketsPathPolicy::CliWithDefault),
            default_testnet_markets_config_path(),
        );
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
