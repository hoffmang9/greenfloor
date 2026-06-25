//! Default config path resolution for the manager CLI.

use std::path::{Path, PathBuf};

pub use crate::paths::{
    default_cats_config_path, default_home_or_repo_config_path, default_markets_config_path,
    default_operator_metadata_config_paths, default_testnet_markets_config_path, expand_home,
    resolve_config_path_from_optional, resolve_repo_root, resolve_vault_scan_testnet_markets_path,
};

const HOME_PROGRAM_CONFIG: &str = "~/.greenfloor/config/program.yaml";
const REPO_PROGRAM_CONFIG: &str = "config/program.yaml";

#[must_use]
pub fn default_program_config_path() -> PathBuf {
    default_home_or_repo_config_path(HOME_PROGRAM_CONFIG, REPO_PROGRAM_CONFIG)
}

#[must_use]
pub fn program_config_path_from_optional(raw: &str) -> PathBuf {
    resolve_config_path_from_optional(raw, default_program_config_path)
}

/// Default metadata paths for ticker index construction (alias).
#[must_use]
pub fn default_metadata_config_paths() -> (PathBuf, PathBuf, Option<PathBuf>) {
    default_operator_metadata_config_paths()
}

/// Vault Coinset scan metadata defaults anchored to the repository `config/` tree.
#[must_use]
pub fn default_vault_scan_metadata_config_paths() -> (PathBuf, PathBuf, Option<PathBuf>) {
    if let Some(repo_root) = resolve_repo_root() {
        return (
            repo_root.join("config/cats.yaml"),
            repo_root.join("config/markets.yaml"),
            resolve_vault_scan_testnet_markets_path(),
        );
    }
    default_metadata_config_paths()
}

pub fn resolve_cli_config_path(
    cli_value: &Path,
    repo_default: &Path,
    home_default: impl FnOnce() -> PathBuf,
) -> PathBuf {
    if cli_value == repo_default {
        let resolved = home_default();
        if resolved != repo_default {
            return resolved;
        }
    }
    cli_value.to_path_buf()
}

#[must_use]
pub fn optional_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::find_repo_root_from;

    #[test]
    fn program_config_path_from_optional_uses_default_when_empty() {
        assert_eq!(
            program_config_path_from_optional(""),
            default_program_config_path()
        );
    }

    #[test]
    fn program_config_path_from_optional_expands_explicit_path() {
        assert_eq!(
            program_config_path_from_optional("~/custom/program.yaml"),
            expand_home(Path::new("~/custom/program.yaml"))
        );
    }

    #[test]
    fn resolve_cli_config_path_uses_home_when_repo_default() {
        let resolved = resolve_cli_config_path(
            Path::new("config/program.yaml"),
            Path::new("config/program.yaml"),
            || PathBuf::from("/tmp/home/program.yaml"),
        );
        assert_eq!(resolved, PathBuf::from("/tmp/home/program.yaml"));
    }

    #[test]
    fn resolve_cli_config_path_keeps_explicit_override() {
        let resolved = resolve_cli_config_path(
            Path::new("/custom/program.yaml"),
            Path::new("config/program.yaml"),
            || PathBuf::from("/tmp/home/program.yaml"),
        );
        assert_eq!(resolved, PathBuf::from("/custom/program.yaml"));
    }

    #[test]
    fn vault_scan_metadata_paths_use_repo_config_when_discoverable() {
        let engine_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = engine_dir.parent().expect("repo root");
        assert_eq!(
            find_repo_root_from(&engine_dir),
            Some(repo_root.to_path_buf())
        );
        let (cats, markets, testnet) = default_vault_scan_metadata_config_paths();
        assert_eq!(cats, repo_root.join("config/cats.yaml"));
        assert_eq!(markets, repo_root.join("config/markets.yaml"));
        assert_eq!(testnet, resolve_vault_scan_testnet_markets_path());
    }
}
