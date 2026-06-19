use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use clap::Parser;
use serde_json::json;

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::config::load_program_config;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::manager_cli::{
    default_cats_config_path, default_markets_config_path, default_metadata_config_paths,
    default_program_config_path, default_testnet_markets_config_path, optional_path,
};
use crate::paths::expand_home;
use crate::vault_coinset_scan::checkpoint::{
    clear_cache_files, read_launcher_id_file, write_launcher_id_file,
};
use crate::vault_coinset_scan::metadata::parse_csv_values;
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::state::run_vault_coinset_scan;
use crate::vault_coinset_scan::types::AssetTypeFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherIdSource {
    Arg,
    File,
    ProgramConfig,
}

impl LauncherIdSource {
    fn label(self) -> &'static str {
        match self {
            Self::Arg => "arg",
            Self::File => "file",
            Self::ProgramConfig => "program_config",
        }
    }
}

#[derive(Debug, Parser)]
pub struct VaultCoinsetScanCliArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub coinset_base_url: String,
    #[arg(long, default_value = "")]
    pub launcher_id: String,
    #[arg(long, default_value = "")]
    pub launcher_id_file: String,
    #[arg(long, default_value = "")]
    pub program_config: String,
    #[arg(long)]
    pub resolve_launcher_id_only: bool,
    #[arg(long, default_value_t = 100)]
    pub max_nonce: u32,
    #[arg(long)]
    pub include_spent: bool,
    #[arg(long, default_value = "all")]
    pub asset_type: String,
    #[arg(long = "cat-id", default_values_t = Vec::<String>::new())]
    pub cat_id: Vec<String>,
    #[arg(long = "cat-ticker", default_values_t = Vec::<String>::new())]
    pub cat_ticker: Vec<String>,
    #[arg(long, default_value = "")]
    pub cat_asset_id: String,
    #[arg(long, default_value = "")]
    pub checkpoint_file: String,
    #[arg(long, default_value_t = 1)]
    pub checkpoint_save_interval: u32,
    #[arg(long)]
    pub no_resume_checkpoint: bool,
    #[arg(long, default_value_t = 32)]
    pub nonce_batch_size: u32,
    #[arg(long, default_value_t = 1)]
    pub empty_batch_stop_count: u32,
    #[arg(long, default_value_t = 64)]
    pub parent_lookup_batch_size: u32,
    #[arg(long)]
    pub start_height: Option<u64>,
    #[arg(long)]
    pub end_height: Option<u64>,
    #[arg(long)]
    pub incremental_from_checkpoint: bool,
    #[arg(long)]
    pub auto_increment: bool,
    #[arg(long)]
    pub clear_caches: bool,
    #[arg(long, default_value = "")]
    pub cats_config: String,
    #[arg(long, default_value = "")]
    pub markets_config: String,
}

impl VaultCoinsetScanCliArgs {
    fn apply_auto_increment_defaults(&mut self) -> SignerResult<()> {
        if !self.auto_increment {
            return Ok(());
        }
        if self.no_resume_checkpoint {
            return Err(SignerError::Other(
                "cannot use --auto-increment with --no-resume-checkpoint".to_string(),
            ));
        }
        if self.checkpoint_file.trim().is_empty() {
            self.checkpoint_file = "~/.greenfloor/cache/vault_coinset_checkpoint.json".to_string();
        }
        self.incremental_from_checkpoint = true;
        Ok(())
    }

    fn cache_clear_paths(&self) -> Option<BTreeMap<String, String>> {
        if !self.clear_caches {
            return None;
        }
        Some(clear_cache_files(&[
            if self.launcher_id_file.trim().is_empty() {
                "~/.greenfloor/cache/vault_launcher_id.txt".to_string()
            } else {
                self.launcher_id_file.clone()
            },
            if self.checkpoint_file.trim().is_empty() {
                "~/.greenfloor/cache/vault_coinset_checkpoint.json".to_string()
            } else {
                self.checkpoint_file.clone()
            },
        ]))
    }

    fn resolve_launcher_id(&self) -> SignerResult<(String, LauncherIdSource)> {
        let from_arg = normalize_hex_id(&self.launcher_id);
        if !from_arg.is_empty() {
            return Ok((from_arg, LauncherIdSource::Arg));
        }
        if !self.launcher_id_file.trim().is_empty() {
            let path = expand_home(std::path::Path::new(self.launcher_id_file.trim()));
            if path.exists() {
                let from_file = read_launcher_id_file(&path)?;
                if from_file.is_empty() {
                    return Err(SignerError::Other(format!(
                        "launcher id file {} is empty",
                        path.display()
                    )));
                }
                return Ok((from_file, LauncherIdSource::File));
            }
        }

        let program_config_path = if self.program_config.trim().is_empty() {
            default_program_config_path()
        } else {
            expand_home(std::path::Path::new(self.program_config.trim()))
        };
        if !program_config_path.exists() {
            return Err(SignerError::Other(
                "launcher-id, launcher-id-file, or --program-config is required".to_string(),
            ));
        }
        let program = load_program_config(&program_config_path)?;
        let launcher = normalize_hex_id(&program.vault_launcher_id);
        if launcher.is_empty() {
            return Err(SignerError::Other(
                "vault_launcher_id_missing_from_program_config".to_string(),
            ));
        }
        Ok((launcher, LauncherIdSource::ProgramConfig))
    }

    fn metadata_config_paths(&self) -> (PathBuf, PathBuf, Option<PathBuf>) {
        if self.cats_config.trim().is_empty() && self.markets_config.trim().is_empty() {
            return default_metadata_config_paths();
        }
        (
            if self.cats_config.trim().is_empty() {
                default_cats_config_path()
            } else {
                expand_home(std::path::Path::new(self.cats_config.trim()))
            },
            if self.markets_config.trim().is_empty() {
                default_markets_config_path()
            } else {
                expand_home(std::path::Path::new(self.markets_config.trim()))
            },
            default_testnet_markets_config_path(),
        )
    }

    fn into_scan_request(
        self,
        launcher_id: String,
        cache_clear: Option<BTreeMap<String, String>>,
    ) -> ScanRequest {
        let (cats_config, markets_config, testnet_markets_config) = self.metadata_config_paths();
        let mut requested_cat_ids = parse_csv_values(&self.cat_id);
        let requested_cat_tickers = parse_csv_values(&self.cat_ticker);
        if !self.cat_asset_id.trim().is_empty() {
            requested_cat_ids.push(self.cat_asset_id.trim().to_string());
        }
        let requested_cat_ids_set = requested_cat_ids
            .iter()
            .filter_map(|value| {
                let normalized = normalize_hex_id(value);
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                }
            })
            .collect::<HashSet<_>>();

        let checkpoint_file =
            optional_path(&self.checkpoint_file).map(|path| expand_home(path.as_path()));

        ScanRequest {
            network: self.network,
            coinset_base_url: optional_trimmed(&self.coinset_base_url),
            launcher_id,
            max_nonce: self.max_nonce,
            include_spent: self.include_spent,
            asset_type: AssetTypeFilter::parse(&self.asset_type),
            requested_cat_ids: requested_cat_ids_set,
            requested_cat_tickers,
            checkpoint_file,
            checkpoint_save_interval: self.checkpoint_save_interval.max(1),
            no_resume_checkpoint: self.no_resume_checkpoint,
            nonce_batch_size: self.nonce_batch_size.max(1),
            empty_batch_stop_count: self.empty_batch_stop_count.max(1),
            parent_lookup_batch_size: self.parent_lookup_batch_size.max(1),
            start_height: self.start_height,
            end_height: self.end_height,
            incremental_from_checkpoint: self.incremental_from_checkpoint,
            auto_increment: self.auto_increment,
            cats_config,
            markets_config,
            testnet_markets_config,
            cache_clear,
        }
    }
}

pub async fn run_vault_coinset_scan_command(args: VaultCoinsetScanCliArgs) -> SignerResult<()> {
    let mut args = args;
    args.apply_auto_increment_defaults()?;

    let cache_clear = args.cache_clear_paths();
    let (launcher_id, launcher_id_source) = args.resolve_launcher_id()?;

    if !args.launcher_id_file.trim().is_empty()
        && matches!(
            launcher_id_source,
            LauncherIdSource::ProgramConfig | LauncherIdSource::Arg
        )
    {
        write_launcher_id_file(
            &expand_home(std::path::Path::new(args.launcher_id_file.trim())),
            &launcher_id,
        )
        .map_err(|err| SignerError::Other(format!("write launcher id file: {err}")))?;
    }

    if args.resolve_launcher_id_only {
        let payload = json!({
            "launcher_id": launcher_id,
            "launcher_id_source": launcher_id_source.label(),
            "launcher_id_file": if args.launcher_id_file.trim().is_empty() {
                None
            } else {
                Some(expand_home(std::path::Path::new(args.launcher_id_file.trim())).display().to_string())
            },
        });
        print_json_value(&payload, true)?;
        return Ok(());
    }

    let request = args.into_scan_request(launcher_id, cache_clear);
    let result = run_vault_coinset_scan(request).await?;
    let payload = serde_json::to_value(result)
        .map_err(|err| SignerError::Other(format!("encode scan result: {err}")))?;
    print_json_value(&payload, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_launcher_id_errors_on_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("launcher.txt");
        std::fs::write(&path, "   \n").expect("write empty launcher file");
        let args = VaultCoinsetScanCliArgs {
            launcher_id_file: path.display().to_string(),
            ..VaultCoinsetScanCliArgs::try_parse_from(["scan"]).expect("parse defaults")
        };
        let err = args.resolve_launcher_id().expect_err("empty launcher file");
        assert!(err.to_string().contains("launcher id file"));
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn parses_vault_coinset_scan_defaults() {
        let args = VaultCoinsetScanCliArgs::try_parse_from(["scan"]).expect("parse defaults");
        assert_eq!(args.network, "mainnet");
        assert_eq!(args.max_nonce, 100);
        assert!(!args.include_spent);
    }
}
