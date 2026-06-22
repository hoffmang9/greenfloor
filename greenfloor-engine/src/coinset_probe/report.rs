use std::future::Future;
use std::path::Path;

use serde_json::Value;

use super::cli::CoinsetProbeCliArgs;
use super::types::{
    CapabilitiesReport, EndpointCapability, NamesCapability, ProbeAttempt, ProbeReport,
};
use crate::cli_util::optional_trimmed;
use crate::coinset::{resolve_direct_client, to_coinset_hex, DirectCoinsetScanClient};
use crate::config::load_program_config;
use crate::error::SignerResult;
use crate::vault::members::{hex_to_bytes32, singleton_member_puzzle_hash, tree_hash_to_hex};
use crate::vault_coinset_scan::launcher::{resolve_launcher_id, ResolveLauncherIdParams};

use super::types::{sample_coin_id_from_records, scan_window_from_peak};

fn program_config_path(program_config: &str) -> std::path::PathBuf {
    use crate::manager_cli::default_program_config_path;
    use crate::paths::expand_home;

    if program_config.trim().is_empty() {
        default_program_config_path()
    } else {
        expand_home(Path::new(program_config))
    }
}

async fn probe_endpoint<FA, RA, FAll, FRange>(
    fetch_all: FAll,
    fetch_range: FRange,
) -> (EndpointCapability, Option<Vec<Value>>)
where
    FAll: FnOnce() -> FA,
    FA: Future<Output = SignerResult<Vec<Value>>>,
    FRange: FnOnce() -> RA,
    RA: Future<Output = SignerResult<Vec<Value>>>,
{
    let (all, records) = ProbeAttempt::run(fetch_all).await;
    let (range, _) = ProbeAttempt::run(fetch_range).await;
    (EndpointCapability::from_attempts(all, range), records)
}

async fn probe_names(
    client: &DirectCoinsetScanClient,
    sample_name: Option<&str>,
    start_height: u64,
    end_height: u64,
) -> NamesCapability {
    let Some(sample_name) = sample_name.filter(|value| !value.is_empty()) else {
        return NamesCapability::skipped();
    };
    let Ok(sample_bytes) = hex_to_bytes32(sample_name) else {
        return NamesCapability::invalid_sample(sample_name, "invalid sample coin id hex");
    };
    let names = vec![to_coinset_hex(sample_bytes.as_ref())];
    let (all, _) = ProbeAttempt::run(|| client.by_names(&names, true, None, None)).await;
    let (range, _) =
        ProbeAttempt::run(|| client.by_names(&names, true, Some(start_height), Some(end_height)))
            .await;
    NamesCapability::from_endpoint(
        sample_name.to_string(),
        EndpointCapability::from_attempts(all, range),
    )
}

/// Build the Coinset capability probe report without emitting output.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn build_coinset_probe_report(args: CoinsetProbeCliArgs) -> SignerResult<ProbeReport> {
    let program_config_path = program_config_path(&args.program_config);
    let preloaded_program = if args.program_config.trim().is_empty() {
        None
    } else {
        Some(load_program_config(&program_config_path)?)
    };
    let program_config_for_resolve = if args.program_config.trim().is_empty() {
        None
    } else {
        Some(program_config_path.as_path())
    };

    let resolved = resolve_launcher_id(&ResolveLauncherIdParams {
        launcher_id: optional_trimmed(&args.launcher_id).as_deref(),
        launcher_id_file: optional_trimmed(&args.launcher_id_file).as_deref(),
        program_config: program_config_for_resolve,
        preloaded_program: preloaded_program.as_ref(),
    })?;

    let launcher_id = hex_to_bytes32(&resolved.launcher_id)?;
    let p2_tree = singleton_member_puzzle_hash(launcher_id, args.nonce)?;
    let p2_hash = tree_hash_to_hex(p2_tree);
    let p2_coinset_hex = to_coinset_hex(p2_tree.to_bytes().as_ref());

    let base_url = optional_trimmed(&args.coinset_base_url);
    let resolved_client = resolve_direct_client(&args.network, base_url.as_deref());
    let network = resolved_client.network.to_string();
    let coinset_base_url = resolved_client.base_url.clone();
    let client = DirectCoinsetScanClient::new(&network, base_url.as_deref());

    let peak_height = client.chain_peak_height().await?.unwrap_or(0);
    let scan_window = scan_window_from_peak(peak_height, args.height_window);
    let start_height = scan_window.start_height;
    let end_height = scan_window.end_height;
    let p2_hashes = [p2_coinset_hex.clone()];

    let ((puzzle_hashes, puzzle_records), (hints, _)) = tokio::join!(
        probe_endpoint(
            || client.by_puzzle_hashes(&p2_hashes, true, None, None),
            || client.by_puzzle_hashes(&p2_hashes, true, Some(start_height), Some(end_height)),
        ),
        probe_endpoint(
            || client.by_hints(&p2_hashes, true, None, None),
            || client.by_hints(&p2_hashes, true, Some(start_height), Some(end_height)),
        ),
    );
    let sample_name = puzzle_records
        .as_deref()
        .and_then(sample_coin_id_from_records);
    let names = probe_names(&client, sample_name.as_deref(), start_height, end_height).await;

    Ok(ProbeReport {
        network,
        coinset_base_url,
        launcher_id: resolved.launcher_id,
        launcher_id_source: resolved.source.label().to_string(),
        probe_nonce: args.nonce,
        probe_p2_hash: p2_hash,
        scan_window,
        capabilities: CapabilitiesReport {
            get_coin_records_by_puzzle_hashes: puzzle_hashes,
            get_coin_records_by_hints: hints,
            get_coin_records_by_names: names,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn build_coinset_probe_report_fails_for_missing_explicit_program_config() {
        let args = CoinsetProbeCliArgs {
            network: "mainnet".to_string(),
            coinset_base_url: String::new(),
            launcher_id: String::new(),
            launcher_id_file: String::new(),
            program_config: "/nonexistent/program.yaml".to_string(),
            nonce: 0,
            height_window: 50_000,
            json: false,
        };
        let err = build_coinset_probe_report(args)
            .await
            .expect_err("missing program config");
        let message = err.to_string();
        assert!(message.contains("failed to read config"));
        assert!(message.contains("nonexistent/program.yaml"));
    }

    #[tokio::test]
    async fn build_coinset_probe_report_with_launcher_id_bypasses_program_config() {
        let launcher = "ab".repeat(32);
        let args = CoinsetProbeCliArgs {
            network: "mainnet".to_string(),
            coinset_base_url: "http://127.0.0.1:1".to_string(),
            launcher_id: launcher,
            launcher_id_file: String::new(),
            program_config: String::new(),
            nonce: 0,
            height_window: 50_000,
            json: false,
        };
        let err = build_coinset_probe_report(args)
            .await
            .expect_err("unreachable coinset");
        let message = err.to_string().to_ascii_lowercase();
        assert!(!message.contains("failed to read config"));
        assert!(
            message.contains("127.0.0.1")
                || message.contains("connection refused")
                || message.contains("error sending request"),
            "expected fast coinset client failure, got: {message}"
        );
    }
}
