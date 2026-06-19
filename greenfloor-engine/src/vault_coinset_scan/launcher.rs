use std::path::{Path, PathBuf};

use crate::config::{load_program_config, ManagerProgramConfig};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::manager_cli::default_program_config_path;
use crate::paths::expand_home;
use crate::vault_coinset_scan::checkpoint::{read_launcher_id_file, write_launcher_id_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherIdSource {
    Arg,
    File,
    ProgramConfig,
}

impl LauncherIdSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Arg => "arg",
            Self::File => "file",
            Self::ProgramConfig => "program_config",
        }
    }
}

pub struct ResolveLauncherIdParams<'a> {
    pub launcher_id: Option<&'a str>,
    pub launcher_id_file: Option<&'a str>,
    pub program_config: Option<&'a Path>,
    /// When set, skips reloading `program_config` for the program-config launcher source.
    pub preloaded_program: Option<&'a ManagerProgramConfig>,
}

#[derive(Debug, Clone)]
pub struct ResolvedLauncherId {
    pub launcher_id: String,
    pub source: LauncherIdSource,
}

/// Resolves a vault launcher id from CLI args, a cache file, or program config.
///
/// # Errors
///
/// Returns an error when no launcher id source is available or the launcher id file is empty.
pub fn resolve_launcher_id(
    params: &ResolveLauncherIdParams<'_>,
) -> SignerResult<ResolvedLauncherId> {
    let from_arg = params
        .launcher_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_hex_id)
        .unwrap_or_default();
    if !from_arg.is_empty() {
        return Ok(ResolvedLauncherId {
            launcher_id: from_arg,
            source: LauncherIdSource::Arg,
        });
    }
    if let Some(path) = params
        .launcher_id_file
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| expand_home(Path::new(value)))
    {
        if path.exists() {
            let from_file = read_launcher_id_file(&path)?;
            if from_file.is_empty() {
                return Err(SignerError::Other(format!(
                    "launcher id file {} is empty",
                    path.display()
                )));
            }
            return Ok(ResolvedLauncherId {
                launcher_id: from_file,
                source: LauncherIdSource::File,
            });
        }
    }

    let program_config_path = params
        .program_config
        .map_or_else(default_program_config_path, Path::to_path_buf);
    if !program_config_path.exists() {
        return Err(SignerError::Other(
            "launcher-id, launcher-id-file, or --program-config is required".to_string(),
        ));
    }
    let loaded;
    let program = if let Some(config) = params.preloaded_program {
        config
    } else {
        loaded = load_program_config(&program_config_path)?;
        &loaded
    };
    let launcher = normalize_hex_id(&program.vault_launcher_id);
    if launcher.is_empty() {
        return Err(SignerError::Other(
            "vault_launcher_id_missing_from_program_config".to_string(),
        ));
    }
    Ok(ResolvedLauncherId {
        launcher_id: launcher,
        source: LauncherIdSource::ProgramConfig,
    })
}

fn launcher_id_file_path(launcher_id_file: Option<&str>) -> Option<PathBuf> {
    launcher_id_file
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| expand_home(Path::new(value)))
}

/// Writes a resolved launcher id to the cache file when the source warrants it.
///
/// # Errors
///
/// Returns an error when the cache file cannot be written.
pub fn cache_resolved_launcher_id(
    launcher_id_file: Option<&str>,
    source: LauncherIdSource,
    launcher_id: &str,
) -> SignerResult<()> {
    let Some(path) = launcher_id_file_path(launcher_id_file) else {
        return Ok(());
    };
    if !matches!(
        source,
        LauncherIdSource::ProgramConfig | LauncherIdSource::Arg
    ) {
        return Ok(());
    }
    write_launcher_id_file(&path, launcher_id)
        .map_err(|err| SignerError::Other(format!("write launcher id file: {err}")))
}
