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
use crate::vault_coinset_scan::request::{LauncherIdSource, ScanRequest};
use crate::vault_coinset_scan::scan::run_vault_coinset_scan;
use crate::vault_coinset_scan::types::AssetTypeFilter;

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

pub async fn run_vault_coinset_scan_command(args: VaultCoinsetScanCliArgs) -> SignerResult<()> {
    let mut args = args;
    if args.auto_increment {
        if args.no_resume_checkpoint {
            return Err(SignerError::Other(
                "cannot use --auto-increment with --no-resume-checkpoint".to_string(),
            ));
        }
        if args.checkpoint_file.trim().is_empty() {
            args.checkpoint_file = "~/.greenfloor/cache/vault_coinset_checkpoint.json".to_string();
        }
        args.incremental_from_checkpoint = true;
    }

    let cache_clear = if args.clear_caches {
        Some(clear_cache_files(&[
            if args.launcher_id_file.trim().is_empty() {
                "~/.greenfloor/cache/vault_launcher_id.txt".to_string()
            } else {
                args.launcher_id_file.clone()
            },
            if args.checkpoint_file.trim().is_empty() {
                "~/.greenfloor/cache/vault_coinset_checkpoint.json".to_string()
            } else {
                args.checkpoint_file.clone()
            },
        ]))
    } else {
        None
    };

    let (launcher_id, launcher_id_source) = resolve_launcher_id(&args)?;
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

    let request = build_scan_request(args, launcher_id, cache_clear);
    let result = run_vault_coinset_scan(request).await?;
    let payload = serde_json::to_value(result)
        .map_err(|err| SignerError::Other(format!("encode scan result: {err}")))?;
    print_json_value(&payload, true)
}

fn build_scan_request(
    args: VaultCoinsetScanCliArgs,
    launcher_id: String,
    cache_clear: Option<BTreeMap<String, String>>,
) -> ScanRequest {
    let (cats_config, markets_config, testnet_markets_config) = resolve_metadata_paths(&args);
    let mut requested_cat_ids = parse_csv_values(&args.cat_id);
    let requested_cat_tickers = parse_csv_values(&args.cat_ticker);
    if !args.cat_asset_id.trim().is_empty() {
        requested_cat_ids.push(args.cat_asset_id.trim().to_string());
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
        optional_path(&args.checkpoint_file).map(|path| expand_home(path.as_path()));

    ScanRequest {
        network: args.network,
        coinset_base_url: optional_trimmed(&args.coinset_base_url),
        launcher_id,
        max_nonce: args.max_nonce,
        include_spent: args.include_spent,
        asset_type: AssetTypeFilter::parse(&args.asset_type),
        requested_cat_ids: requested_cat_ids_set,
        requested_cat_tickers,
        checkpoint_file,
        checkpoint_save_interval: args.checkpoint_save_interval.max(1),
        no_resume_checkpoint: args.no_resume_checkpoint,
        nonce_batch_size: args.nonce_batch_size.max(1),
        empty_batch_stop_count: args.empty_batch_stop_count.max(1),
        parent_lookup_batch_size: args.parent_lookup_batch_size.max(1),
        start_height: args.start_height,
        end_height: args.end_height,
        incremental_from_checkpoint: args.incremental_from_checkpoint,
        auto_increment: args.auto_increment,
        cats_config,
        markets_config,
        testnet_markets_config,
        cache_clear,
    }
}

fn resolve_metadata_paths(args: &VaultCoinsetScanCliArgs) -> (PathBuf, PathBuf, Option<PathBuf>) {
    if args.cats_config.trim().is_empty() && args.markets_config.trim().is_empty() {
        return default_metadata_config_paths();
    }
    (
        if args.cats_config.trim().is_empty() {
            default_cats_config_path()
        } else {
            expand_home(std::path::Path::new(args.cats_config.trim()))
        },
        if args.markets_config.trim().is_empty() {
            default_markets_config_path()
        } else {
            expand_home(std::path::Path::new(args.markets_config.trim()))
        },
        default_testnet_markets_config_path(),
    )
}

fn resolve_launcher_id(args: &VaultCoinsetScanCliArgs) -> SignerResult<(String, LauncherIdSource)> {
    let from_arg = normalize_hex_id(&args.launcher_id);
    if !from_arg.is_empty() {
        return Ok((from_arg, LauncherIdSource::Arg));
    }
    if !args.launcher_id_file.trim().is_empty() {
        let from_file = read_launcher_id_file(&expand_home(std::path::Path::new(
            args.launcher_id_file.trim(),
        )));
        if !from_file.is_empty() {
            return Ok((from_file, LauncherIdSource::File));
        }
    }

    let program_config_path = if args.program_config.trim().is_empty() {
        default_program_config_path()
    } else {
        expand_home(std::path::Path::new(args.program_config.trim()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vault_coinset_scan_defaults() {
        let args = VaultCoinsetScanCliArgs::try_parse_from(["scan"]).expect("parse defaults");
        assert_eq!(args.network, "mainnet");
        assert_eq!(args.max_nonce, 100);
        assert!(!args.include_spent);
    }
}
