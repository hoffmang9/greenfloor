use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use clap::Parser;
use serde_json::json;

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::manager_cli::{
    default_program_config_path, default_testnet_markets_config_path,
    default_vault_scan_metadata_config_paths, optional_path,
};
use crate::paths::expand_home;
use crate::vault_coinset_scan::launcher::{
    cache_resolved_launcher_id, resolve_launcher_id, ResolveLauncherIdParams,
};
use crate::vault_coinset_scan::metadata::parse_csv_values;
use crate::vault_coinset_scan::request::{ScanCheckpointControl, ScanRequest};
use crate::vault_coinset_scan::state::ScanState;
use crate::vault_coinset_scan::types::AssetTypeFilter;

#[must_use]
fn clear_cache_files(paths: &[String]) -> BTreeMap<String, String> {
    let mut results = BTreeMap::new();
    for raw_path in paths {
        let clean = raw_path.trim();
        if clean.is_empty() {
            continue;
        }
        let path = expand_home(std::path::Path::new(clean));
        let key = path.display().to_string();
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    results.insert(key, "deleted".to_string());
                }
                Err(err) => {
                    results.insert(key, format!("delete_failed:{err}"));
                }
            }
        } else {
            results.insert(key, "not_found".to_string());
        }
    }
    results
}

#[derive(Debug, Parser)]
pub struct VaultCoinsetScanCheckpointCli {
    #[arg(long)]
    pub no_resume_checkpoint: bool,
    #[arg(long)]
    pub incremental_from_checkpoint: bool,
    #[arg(long)]
    pub auto_increment: bool,
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
    #[command(flatten)]
    pub checkpoint: VaultCoinsetScanCheckpointCli,
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
    pub clear_caches: bool,
    #[arg(long, default_value = "")]
    pub cats_config: String,
    #[arg(long, default_value = "")]
    pub markets_config: String,
}

impl VaultCoinsetScanCliArgs {
    fn apply_auto_increment_defaults(&mut self) -> SignerResult<()> {
        if !self.checkpoint.auto_increment {
            return Ok(());
        }
        if self.checkpoint.no_resume_checkpoint {
            return Err(SignerError::Other(
                "cannot use --auto-increment with --no-resume-checkpoint".to_string(),
            ));
        }
        if self.checkpoint_file.trim().is_empty() {
            self.checkpoint_file = "~/.greenfloor/cache/vault_coinset_checkpoint.json".to_string();
        }
        self.checkpoint.incremental_from_checkpoint = true;
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

    fn program_config_path(&self) -> PathBuf {
        if self.program_config.trim().is_empty() {
            default_program_config_path()
        } else {
            expand_home(std::path::Path::new(self.program_config.trim()))
        }
    }

    fn metadata_config_paths(&self) -> (PathBuf, PathBuf, Option<PathBuf>) {
        let (default_cats, default_markets, default_testnet) =
            default_vault_scan_metadata_config_paths();
        (
            if self.cats_config.trim().is_empty() {
                default_cats
            } else {
                expand_home(std::path::Path::new(self.cats_config.trim()))
            },
            if self.markets_config.trim().is_empty() {
                default_markets
            } else {
                expand_home(std::path::Path::new(self.markets_config.trim()))
            },
            default_testnet.or_else(default_testnet_markets_config_path),
        )
    }

    fn into_scan_request(
        self,
        launcher_id: String,
        cache_clear: Option<BTreeMap<String, String>>,
    ) -> crate::vault_coinset_scan::ScanRequest {
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

        scan_request_from_cli_args(
            self,
            launcher_id,
            requested_cat_ids_set,
            requested_cat_tickers,
            checkpoint_file,
            cache_clear,
        )
    }
}

fn scan_request_from_cli_args(
    args: VaultCoinsetScanCliArgs,
    launcher_id: String,
    requested_cat_ids: HashSet<String>,
    requested_cat_tickers: Vec<String>,
    checkpoint_file: Option<PathBuf>,
    cache_clear: Option<BTreeMap<String, String>>,
) -> ScanRequest {
    let (cats_config, markets_config, testnet_markets_config) = args.metadata_config_paths();
    ScanRequest {
        network: args.network,
        coinset_base_url: optional_trimmed(&args.coinset_base_url),
        launcher_id,
        max_nonce: args.max_nonce,
        include_spent: args.include_spent,
        asset_type: AssetTypeFilter::parse(&args.asset_type),
        requested_cat_ids,
        requested_cat_tickers,
        checkpoint_file,
        checkpoint_save_interval: args.checkpoint_save_interval.max(1),
        checkpoint: ScanCheckpointControl {
            no_resume_checkpoint: args.checkpoint.no_resume_checkpoint,
            incremental_from_checkpoint: args.checkpoint.incremental_from_checkpoint,
            auto_increment: args.checkpoint.auto_increment,
        },
        nonce_batch_size: args.nonce_batch_size.max(1),
        empty_batch_stop_count: args.empty_batch_stop_count.max(1),
        parent_lookup_batch_size: args.parent_lookup_batch_size.max(1),
        start_height: args.start_height,
        end_height: args.end_height,
        cats_config,
        markets_config,
        testnet_markets_config,
        cache_clear,
    }
}

/// Run vault coinset scan command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_vault_coinset_scan_command(args: VaultCoinsetScanCliArgs) -> SignerResult<()> {
    let mut args = args;
    args.apply_auto_increment_defaults()?;

    let cache_clear = args.cache_clear_paths();
    let resolved = resolve_launcher_id(&ResolveLauncherIdParams {
        launcher_id: optional_trimmed(&args.launcher_id).as_deref(),
        launcher_id_file: optional_trimmed(&args.launcher_id_file).as_deref(),
        program_config: Some(args.program_config_path().as_path()),
        preloaded_program: None,
    })?;
    let launcher_id = resolved.launcher_id;
    let launcher_id_source = resolved.source;

    cache_resolved_launcher_id(
        Some(args.launcher_id_file.trim()),
        launcher_id_source,
        &launcher_id,
    )?;

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
    let result = ScanState::run(request).await?;
    let payload = serde_json::to_value(result)
        .map_err(|err| SignerError::Other(format!("encode scan result: {err}")))?;
    print_json_value(&payload, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_coinset_scan::launcher::ResolveLauncherIdParams;

    #[test]
    fn resolve_launcher_id_errors_on_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("launcher.txt");
        std::fs::write(&path, "   \n").expect("write empty launcher file");
        let err = resolve_launcher_id(&ResolveLauncherIdParams {
            launcher_id: None,
            launcher_id_file: Some(path.to_str().expect("launcher path")),
            program_config: None,
            preloaded_program: None,
        })
        .expect_err("empty launcher file");
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

    #[test]
    fn clear_cache_files_reports_missing_and_deleted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let existing = dir.path().join("exists.txt");
        std::fs::write(&existing, "x").expect("write file");
        let missing = dir.path().join("missing.txt");
        let results = super::clear_cache_files(&[
            existing.display().to_string(),
            missing.display().to_string(),
        ]);
        assert_eq!(
            results.get(&existing.display().to_string()),
            Some(&"deleted".to_string())
        );
        assert_eq!(
            results.get(&missing.display().to_string()),
            Some(&"not_found".to_string())
        );
    }
}
