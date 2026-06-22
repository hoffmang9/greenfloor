//! Canonical path helpers shared across config, manager CLI, and daemon.
//!
//! ## Testnet markets overlay resolution
//!
//! Three entrypoint contracts — do not add a fourth resolver without updating this list:
//!
//! - [`resolve_testnet_markets_path`] with [`TestnetMarketsPathPolicy::RequireExistingFile`]:
//!   daemon CLI / JSON once requests. Empty flag → `None`. Explicit path → `Some` only when
//!   [`expand_home`] path exists on disk.
//! - [`resolve_testnet_markets_path`] with [`TestnetMarketsPathPolicy::CliWithDefault`]:
//!   manager CLI. Empty flag → [`default_testnet_markets_config_path`] (home file when present).
//!   Explicit path → always `Some` after [`expand_home`] (no exists check).
//! - [`resolve_vault_scan_testnet_markets_path`]: vault coinset scan metadata defaults. Repo
//!   `config/testnet-markets.yaml` when discoverable and present, else home default.

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

/// Prefer `home_relative` when that file exists; otherwise use `repo_relative` (cwd-relative).
#[must_use]
pub fn default_home_or_repo_config_path(home_relative: &str, repo_relative: &str) -> PathBuf {
    let home_default = expand_home(Path::new(home_relative));
    if home_default.exists() {
        home_default
    } else {
        PathBuf::from(repo_relative)
    }
}

/// Resolve an optional CLI path: empty uses `default`, explicit values expand `~`.
#[must_use]
pub fn resolve_config_path_from_optional(raw: &str, default: impl FnOnce() -> PathBuf) -> PathBuf {
    if raw.trim().is_empty() {
        default()
    } else {
        expand_home(Path::new(raw.trim()))
    }
}

const DEFAULT_TESTNET_MARKETS_CONFIG: &str = "~/.greenfloor/config/testnet-markets.yaml";

/// Default testnet markets overlay when `~/.greenfloor/config/testnet-markets.yaml` exists.
#[must_use]
pub fn default_testnet_markets_config_path() -> Option<PathBuf> {
    let home_default = expand_home(Path::new(DEFAULT_TESTNET_MARKETS_CONFIG));
    home_default.exists().then_some(home_default)
}

/// Vault scan testnet overlay: repo `config/testnet-markets.yaml` when present, else home default.
#[must_use]
pub fn resolve_vault_scan_testnet_markets_path() -> Option<PathBuf> {
    if let Some(repo_root) = resolve_repo_root() {
        let testnet = repo_root.join("config/testnet-markets.yaml");
        if testnet.exists() {
            return Some(testnet);
        }
    }
    default_testnet_markets_config_path()
}

/// How operator entrypoints resolve `--testnet-markets-config` / `testnet_markets_path`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestnetMarketsPathPolicy {
    /// Daemon CLI: empty → `None`; explicit path → `Some` only when [`expand_home`] path exists.
    RequireExistingFile,
    /// Manager CLI: empty → [`default_testnet_markets_config_path`]; explicit path → `Some` after [`expand_home`].
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

    let path = expand_home(Path::new(trimmed));
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
    fn resolve_testnet_markets_path_expands_explicit_tilde_paths() {
        if let Ok(home) = std::env::var("HOME") {
            let expected = PathBuf::from(home).join("custom-testnet-markets.yaml");
            assert_eq!(
                resolve_testnet_markets_path(
                    "~/custom-testnet-markets.yaml",
                    TestnetMarketsPathPolicy::CliWithDefault,
                ),
                Some(expected.clone())
            );
            assert_eq!(
                resolve_testnet_markets_path(
                    "~/custom-testnet-markets.yaml",
                    TestnetMarketsPathPolicy::RequireExistingFile,
                ),
                expected.exists().then_some(expected)
            );
        }
    }

    #[test]
    fn resolve_vault_scan_testnet_markets_path_prefers_repo_then_home() {
        let engine_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = engine_dir.parent().expect("repo root");
        let repo_testnet = repo_root.join("config/testnet-markets.yaml");
        if repo_testnet.is_file() {
            assert_eq!(
                resolve_vault_scan_testnet_markets_path(),
                Some(repo_testnet)
            );
        } else {
            assert_eq!(
                resolve_vault_scan_testnet_markets_path(),
                default_testnet_markets_config_path(),
            );
        }
    }

    #[test]
    fn resolve_config_path_from_optional_expands_explicit_path() {
        assert_eq!(
            resolve_config_path_from_optional("/custom/program.yaml", || {
                PathBuf::from("config/program.yaml")
            }),
            PathBuf::from("/custom/program.yaml")
        );
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(
                resolve_config_path_from_optional("~/custom/program.yaml", || {
                    PathBuf::from("config/program.yaml")
                }),
                PathBuf::from(home).join("custom/program.yaml")
            );
        }
    }

    #[test]
    fn default_home_or_repo_config_path_prefers_existing_home_file() {
        let resolved = default_home_or_repo_config_path(
            "~/.greenfloor/config/program.yaml",
            "config/program.yaml",
        );
        let home = expand_home(Path::new("~/.greenfloor/config/program.yaml"));
        if home.exists() {
            assert_eq!(resolved, home);
        } else {
            assert_eq!(resolved, PathBuf::from("config/program.yaml"));
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

    #[test]
    fn resolve_repo_root_honors_greenfloor_repo_root_env() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_dir = dir.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        std::fs::write(config_dir.join("cats.yaml"), "cats: []\n").expect("write cats");
        let _guard = crate::test_env::EnvRestoreGuard::set(&[(
            "GREENFLOOR_REPO_ROOT",
            dir.path().to_str().expect("repo path"),
        )]);
        assert_eq!(resolve_repo_root(), Some(dir.path().to_path_buf()));
    }

    #[test]
    fn resolve_repo_root_ignores_invalid_greenfloor_repo_root_env() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_dir = dir.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        std::fs::write(config_dir.join("cats.yaml"), "cats: []\n").expect("write cats");
        let _guard = crate::test_env::EnvRestoreGuard::set(&[(
            "GREENFLOOR_REPO_ROOT",
            "/nonexistent/greenfloor-repo-root",
        )]);
        let resolved = resolve_repo_root();
        let engine_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(resolved, find_repo_root_from(&engine_dir));
        let _ = dir;
    }
}
