//! Default config path resolution for the manager CLI.

use std::path::{Path, PathBuf};

pub use crate::paths::{expand_home, resolve_repo_root};

#[must_use]
pub fn default_program_config_path() -> PathBuf {
    let home_default = expand_home(Path::new("~/.greenfloor/config/program.yaml"));
    if home_default.exists() {
        return home_default;
    }
    PathBuf::from("config/program.yaml")
}

#[must_use]
pub fn default_markets_config_path() -> PathBuf {
    let home_default = expand_home(Path::new("~/.greenfloor/config/markets.yaml"));
    if home_default.exists() {
        return home_default;
    }
    PathBuf::from("config/markets.yaml")
}

#[must_use]
pub fn default_testnet_markets_config_path() -> Option<PathBuf> {
    let home_default = expand_home(Path::new("~/.greenfloor/config/testnet-markets.yaml"));
    if home_default.exists() {
        return Some(home_default);
    }
    None
}

#[must_use]
pub fn default_cats_config_path() -> PathBuf {
    let home_default = expand_home(Path::new("~/.greenfloor/config/cats.yaml"));
    if home_default.exists() {
        return home_default;
    }
    PathBuf::from("config/cats.yaml")
}

#[must_use]
pub fn default_metadata_config_paths() -> (PathBuf, PathBuf, Option<PathBuf>) {
    (
        default_cats_config_path(),
        default_markets_config_path(),
        default_testnet_markets_config_path(),
    )
}

/// Vault Coinset scan metadata defaults anchored to the repository `config/` tree.
///
/// Matches the legacy Python scanner (`Path(__file__).parents[1] / "config"`), so scans
/// invoked outside the repo cwd still resolve ticker indexes from the checkout.
#[must_use]
pub fn default_vault_scan_metadata_config_paths() -> (PathBuf, PathBuf, Option<PathBuf>) {
    if let Some(repo_root) = resolve_repo_root() {
        let testnet = repo_root.join("config/testnet-markets.yaml");
        return (
            repo_root.join("config/cats.yaml"),
            repo_root.join("config/markets.yaml"),
            testnet.exists().then_some(testnet),
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
        let (cats, markets, _) = default_vault_scan_metadata_config_paths();
        assert_eq!(cats, repo_root.join("config/cats.yaml"));
        assert_eq!(markets, repo_root.join("config/markets.yaml"));
    }
}
