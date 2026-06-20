//! Probe Coinset endpoint height-window capabilities for vault scans.

use std::future::Future;
use std::path::Path;

use clap::Parser;
use serde::Serialize;
use serde_json::Value;

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::coinset::{
    coin_id_from_record, resolve_direct_client, to_coinset_hex, DirectCoinsetScanClient,
};
use crate::config::load_program_config;
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::default_program_config_path;
use crate::paths::expand_home;
use crate::vault::members::{hex_to_bytes32, singleton_member_puzzle_hash, tree_hash_to_hex};
use crate::vault_coinset_scan::launcher::{resolve_launcher_id, ResolveLauncherIdParams};

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProbeAttempt {
    pub supported: bool,
    pub error: Option<String>,
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EndpointCapability {
    pub all_supported: bool,
    pub all_error: Option<String>,
    pub all_count: Option<usize>,
    pub range_supported: bool,
    pub range_error: Option<String>,
    pub range_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NamesCapability {
    pub sample_name: Option<String>,
    pub all_supported: Option<bool>,
    pub all_error: Option<String>,
    pub all_count: Option<usize>,
    pub range_supported: Option<bool>,
    pub range_error: Option<String>,
    pub range_count: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ProbeReport {
    pub network: String,
    pub coinset_base_url: String,
    pub launcher_id: String,
    pub launcher_id_source: String,
    pub probe_nonce: u32,
    pub probe_p2_hash: String,
    pub scan_window: ScanWindow,
    pub capabilities: CapabilitiesReport,
}

#[derive(Debug, Serialize)]
pub struct ScanWindow {
    pub start_height: u64,
    pub end_height: u64,
    pub peak_height: u64,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct CapabilitiesReport {
    pub get_coin_records_by_puzzle_hashes: EndpointCapability,
    pub get_coin_records_by_hints: EndpointCapability,
    pub get_coin_records_by_names: NamesCapability,
}

impl ProbeAttempt {
    pub async fn run<F, Fut>(fetch: F) -> (Self, Option<Vec<Value>>)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = SignerResult<Vec<Value>>>,
    {
        match fetch().await {
            Ok(records) => {
                let count = records.len();
                (
                    Self {
                        supported: true,
                        error: None,
                        count: Some(count),
                    },
                    Some(records),
                )
            }
            Err(err) => (
                Self {
                    supported: false,
                    error: Some(err.to_string()),
                    count: None,
                },
                None,
            ),
        }
    }
}

impl EndpointCapability {
    #[must_use]
    pub fn from_attempts(all: ProbeAttempt, range: ProbeAttempt) -> Self {
        Self {
            all_supported: all.supported,
            all_error: all.error,
            all_count: all.count,
            range_supported: range.supported,
            range_error: range.error,
            range_count: range.count,
        }
    }
}

impl NamesCapability {
    #[must_use]
    pub fn skipped() -> Self {
        Self {
            sample_name: None,
            all_supported: None,
            all_error: None,
            all_count: None,
            range_supported: None,
            range_error: None,
            range_count: None,
        }
    }

    #[must_use]
    pub fn invalid_sample(sample_name: &str, message: &str) -> Self {
        Self {
            sample_name: Some(sample_name.to_string()),
            all_supported: Some(false),
            all_error: Some(message.to_string()),
            all_count: None,
            range_supported: Some(false),
            range_error: Some(message.to_string()),
            range_count: None,
        }
    }

    #[must_use]
    pub fn from_endpoint(sample_name: String, endpoint: EndpointCapability) -> Self {
        Self {
            sample_name: Some(sample_name),
            all_supported: Some(endpoint.all_supported),
            all_error: endpoint.all_error,
            all_count: endpoint.all_count,
            range_supported: Some(endpoint.range_supported),
            range_error: endpoint.range_error,
            range_count: endpoint.range_count,
        }
    }
}

fn program_config_path(program_config: &str) -> std::path::PathBuf {
    if program_config.trim().is_empty() {
        default_program_config_path()
    } else {
        expand_home(Path::new(program_config))
    }
}

#[must_use]
pub fn sample_coin_id_from_records(records: &[Value]) -> Option<String> {
    for record in records {
        let coin_id = coin_id_from_record(record);
        if !coin_id.is_empty() {
            return Some(coin_id);
        }
    }
    None
}

#[must_use]
pub fn scan_window_from_peak(peak_height: u64, height_window: u64) -> ScanWindow {
    let height_window = height_window.max(1);
    let start_height = peak_height.saturating_sub(height_window);
    ScanWindow {
        start_height,
        end_height: peak_height,
        peak_height,
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
    fn scan_window_from_peak_applies_height_window() {
        let window = scan_window_from_peak(12_345, 50_000);
        assert_eq!(window.peak_height, 12_345);
        assert_eq!(window.end_height, 12_345);
        assert_eq!(window.start_height, 0);
    }

    #[test]
    fn scan_window_from_peak_subtracts_window_from_peak() {
        let window = scan_window_from_peak(100_000, 10_000);
        assert_eq!(window.start_height, 90_000);
        assert_eq!(window.end_height, 100_000);
    }

    #[test]
    fn sample_coin_id_from_records_prefers_first_resolvable_record() {
        let records = vec![
            serde_json::json!({"coin": {"amount": 1}}),
            serde_json::json!({
                "coin": {
                    "parent_coin_info": format!("0x{}", "a".repeat(64)),
                    "puzzle_hash": format!("0x{}", "b".repeat(64)),
                    "amount": 2
                }
            }),
        ];
        let sample = sample_coin_id_from_records(&records).expect("sample");
        assert_eq!(sample.len(), 64);
    }

    #[test]
    fn endpoint_capability_from_attempts_maps_fields() {
        let capability = EndpointCapability::from_attempts(
            ProbeAttempt {
                supported: true,
                error: None,
                count: Some(3),
            },
            ProbeAttempt {
                supported: false,
                error: Some("range failed".to_string()),
                count: None,
            },
        );
        assert!(capability.all_supported);
        assert_eq!(capability.all_count, Some(3));
        assert!(!capability.range_supported);
        assert_eq!(capability.range_error.as_deref(), Some("range failed"));
    }

    #[test]
    fn names_capability_skipped_serializes_null_fields() {
        let payload = serde_json::to_value(NamesCapability::skipped()).expect("json");
        assert!(payload.get("sample_name").unwrap().is_null());
        assert!(payload.get("all_supported").unwrap().is_null());
    }

    #[test]
    fn parses_coinset_probe_defaults() {
        let args = CoinsetProbeCliArgs::try_parse_from(["probe"]).expect("parse");
        assert_eq!(args.network, "mainnet");
        assert_eq!(args.nonce, 0);
        assert_eq!(args.height_window, 50_000);
        assert!(!args.json);
    }

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

    #[tokio::test]
    async fn probe_attempt_run_maps_success_and_failure() {
        use crate::error::SignerError;

        let (ok, records) =
            ProbeAttempt::run(|| async { Ok(vec![serde_json::json!({"coin": {"amount": 1}})]) })
                .await;
        assert!(ok.supported);
        assert_eq!(ok.count, Some(1));
        assert_eq!(records.as_ref().map(Vec::len), Some(1));

        let (err, records) = ProbeAttempt::run(|| async {
            Err::<Vec<serde_json::Value>, _>(SignerError::Other("boom".to_string()))
        })
        .await;
        assert!(!err.supported);
        assert_eq!(err.error.as_deref(), Some("boom"));
        assert!(records.is_none());
    }
}
