use std::path::PathBuf;

use clap::Parser;

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::config::load_program_config;
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::program_config_path_from_optional;
use crate::vault_coinset_scan::launcher::{
    resolve_launcher_id, ResolveLauncherIdParams, ResolvedLauncherId,
};

use super::report::build_coinset_probe_report;

#[derive(Debug, Parser)]
pub struct CoinsetProbeCliArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub coinset_base_url: String,
    #[arg(long, default_value = "")]
    pub launcher_id: String,
    #[arg(long, default_value = "")]
    pub launcher_id_file: String,
    #[arg(
        long,
        default_value = "",
        help = "Path to program.yaml used to resolve vault.launcher_id when --launcher-id is omitted."
    )]
    pub program_config: String,
    #[arg(long, default_value_t = 0, help = "Member nonce to probe (default 0).")]
    pub nonce: u32,
    #[arg(
        long,
        default_value_t = 50_000,
        help = "Probe range window in blocks from chain peak (default 50000)."
    )]
    pub height_window: u64,
    #[arg(long)]
    pub json: bool,
}

impl CoinsetProbeCliArgs {
    #[must_use]
    pub fn program_config_path(&self) -> PathBuf {
        program_config_path_from_optional(&self.program_config)
    }

    /// Resolve launcher id from CLI args, cache file, or explicit program config.
    ///
    /// # Errors
    ///
    /// Returns an error if launcher resolution fails.
    pub fn resolve_launcher_id(&self) -> SignerResult<ResolvedLauncherId> {
        let explicit_program_config = self.explicit_program_config_path();
        let mut loaded_program = None;
        let program_config = explicit_program_config.as_deref();
        if let Some(path) = program_config {
            loaded_program = Some(load_program_config(path)?);
        }
        resolve_launcher_id(&ResolveLauncherIdParams {
            launcher_id: optional_trimmed(&self.launcher_id).as_deref(),
            launcher_id_file: optional_trimmed(&self.launcher_id_file).as_deref(),
            program_config,
            preloaded_program: loaded_program.as_ref(),
        })
    }

    fn explicit_program_config_path(&self) -> Option<PathBuf> {
        if self.program_config.trim().is_empty() {
            None
        } else {
            Some(self.program_config_path())
        }
    }
}

/// Probe Coinset height-window API support for vault scans and emit a JSON report.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_coinset_probe_command(args: CoinsetProbeCliArgs) -> SignerResult<()> {
    let json = args.json;
    let report = build_coinset_probe_report(args).await?;

    if json {
        print_json_value(
            &serde_json::to_value(&report)
                .map_err(|err| SignerError::Other(format!("json encode failed: {err}")))?,
            true,
        )?;
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|err| SignerError::Other(format!("json encode failed: {err}")))?
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_coinset_probe_defaults() {
        let args = CoinsetProbeCliArgs::try_parse_from(["probe"]).expect("parse");
        assert_eq!(args.network, "mainnet");
        assert_eq!(args.nonce, 0);
        assert_eq!(args.height_window, 50_000);
        assert!(!args.json);
    }
}
