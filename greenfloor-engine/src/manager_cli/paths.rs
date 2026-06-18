//! Default config path resolution for the manager CLI.

use std::path::{Path, PathBuf};

pub fn expand_home(path: &Path) -> PathBuf {
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

pub fn default_program_config_path() -> PathBuf {
    let home_default = expand_home(Path::new("~/.greenfloor/config/program.yaml"));
    if home_default.exists() {
        return home_default;
    }
    PathBuf::from("config/program.yaml")
}

pub fn default_markets_config_path() -> PathBuf {
    let home_default = expand_home(Path::new("~/.greenfloor/config/markets.yaml"));
    if home_default.exists() {
        return home_default;
    }
    PathBuf::from("config/markets.yaml")
}

pub fn default_testnet_markets_config_path() -> Option<PathBuf> {
    let home_default = expand_home(Path::new("~/.greenfloor/config/testnet-markets.yaml"));
    if home_default.exists() {
        return Some(home_default);
    }
    None
}

pub fn default_cats_config_path() -> PathBuf {
    let home_default = expand_home(Path::new("~/.greenfloor/config/cats.yaml"));
    if home_default.exists() {
        return home_default;
    }
    PathBuf::from("config/cats.yaml")
}

pub fn default_state_dir_path() -> PathBuf {
    expand_home(Path::new("~/.greenfloor/state"))
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
}
