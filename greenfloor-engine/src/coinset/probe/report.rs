use super::capability::{
    probe_height_window, probe_names, sample_coin_id_from_records, scan_window_from_peak,
};
use super::cli::CoinsetProbeCliArgs;
use super::types::{CapabilitiesReport, ProbeReport};
use crate::cli_util::optional_trimmed;
use crate::coinset::{to_coinset_hex, DirectCoinsetScanClient};
use crate::error::SignerResult;
use crate::vault::members::{hex_to_bytes32, singleton_member_puzzle_hash, tree_hash_to_hex};

/// Build the Coinset capability probe report without emitting output.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn build_coinset_probe_report(args: CoinsetProbeCliArgs) -> SignerResult<ProbeReport> {
    let resolved = args.resolve_launcher_id()?;

    let launcher_id = hex_to_bytes32(&resolved.launcher_id)?;
    let p2_tree = singleton_member_puzzle_hash(launcher_id, args.nonce)?;
    let p2_hash = tree_hash_to_hex(p2_tree);
    let p2_coinset_hex = to_coinset_hex(p2_tree.to_bytes().as_ref());

    let base_url = optional_trimmed(&args.coinset_base_url);
    let resolved_client = DirectCoinsetScanClient::resolve(&args.network, base_url.as_deref());
    let client = &resolved_client.client;

    let peak_height = client.chain_peak_height().await?.unwrap_or(0);
    let scan_window = scan_window_from_peak(peak_height, args.height_window);
    let start_height = scan_window.start_height;
    let end_height = scan_window.end_height;
    let p2_hashes = [p2_coinset_hex];

    let ((puzzle_hashes, puzzle_records), (hints, _)) = tokio::join!(
        probe_height_window(start_height, end_height, None, |start, end| {
            client.by_puzzle_hashes(&p2_hashes, true, start, end)
        }),
        probe_height_window(start_height, end_height, None, |start, end| {
            client.by_hints(&p2_hashes, true, start, end)
        }),
    );
    let sample_name = puzzle_records
        .as_deref()
        .and_then(sample_coin_id_from_records);
    let names = probe_names(client, sample_name.as_deref(), start_height, end_height).await;

    Ok(ProbeReport {
        network: resolved_client.network,
        coinset_base_url: resolved_client.base_url,
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
