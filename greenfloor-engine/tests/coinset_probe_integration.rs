//! Mocked Coinset integration tests for `greenfloor-engine coinset probe`.

#[path = "fixtures/coinset_probe_mocks.rs"]
mod coinset_probe_mocks;

use coinset_probe_mocks::{mount_probe_server, HttpMockBody, NamesMockMode, ProbeServerMockConfig};
use greenfloor_engine::coinset::to_coinset_hex;
use greenfloor_engine::coinset_probe::{build_coinset_probe_report, CoinsetProbeCliArgs};
use greenfloor_engine::hex::normalize_hex_id;
use greenfloor_engine::vault::members::{
    hex_to_bytes32, singleton_member_puzzle_hash_hex_from_launcher_id,
};

fn launcher_id() -> String {
    "ab".repeat(32)
}

fn p2_coinset_hex(launcher: &str, nonce: u32) -> String {
    let hash = singleton_member_puzzle_hash_hex_from_launcher_id(launcher, nonce).expect("p2 hash");
    let bytes = hex_to_bytes32(&hash).expect("p2 bytes");
    to_coinset_hex(bytes.as_ref())
}

fn probe_args(server_url: String, launcher: String, height_window: u64) -> CoinsetProbeCliArgs {
    CoinsetProbeCliArgs {
        network: "mainnet".to_string(),
        coinset_base_url: server_url,
        launcher_id: launcher,
        launcher_id_file: String::new(),
        program_config: String::new(),
        nonce: 0,
        height_window,
        json: true,
    }
}

fn assert_operator_probe_report_contract(
    report: &greenfloor_engine::coinset_probe::ProbeReport,
    mocks: &ProbeServerMockConfig,
    launcher: &str,
    expected_p2_hash: &str,
) {
    assert_eq!(report.network, "mainnet");
    assert_eq!(report.launcher_id, launcher);
    assert_eq!(report.launcher_id_source, "arg");
    assert_eq!(report.probe_nonce, 0);
    assert_eq!(report.probe_p2_hash, expected_p2_hash);
    assert_eq!(report.scan_window.peak_height, mocks.peak_height);
    assert_eq!(report.scan_window.end_height, mocks.peak_height);
    assert_eq!(report.scan_window.start_height, 0);
    assert!(
        report
            .capabilities
            .get_coin_records_by_puzzle_hashes
            .all_supported
    );
    assert_eq!(
        report
            .capabilities
            .get_coin_records_by_puzzle_hashes
            .all_count,
        Some(1)
    );
    assert!(report.capabilities.get_coin_records_by_hints.all_supported);
    match &mocks.names {
        NamesMockMode::WithSample(sample_coin_id) => {
            assert_eq!(
                report
                    .capabilities
                    .get_coin_records_by_names
                    .sample_name
                    .as_deref(),
                Some(normalize_hex_id(sample_coin_id).as_str())
            );
            assert_eq!(
                report.capabilities.get_coin_records_by_names.all_count,
                Some(0)
            );
        }
        NamesMockMode::Skip => panic!("operator contract mock must mount names sample"),
    }

    let payload = serde_json::to_value(report).expect("json");
    assert!(payload.get("network").is_some());
    assert!(payload.get("coinset_base_url").is_some());
    assert!(payload.get("capabilities").is_some());
    assert!(payload["capabilities"]
        .get("get_coin_records_by_puzzle_hashes")
        .is_some());
}

#[tokio::test]
async fn build_coinset_probe_report_matches_operator_json_contract() {
    let launcher = launcher_id();
    let p2_hex = p2_coinset_hex(&launcher, 0);
    let sample_coin_id = "cd".repeat(32);
    let expected_p2_hash =
        singleton_member_puzzle_hash_hex_from_launcher_id(&launcher, 0).expect("p2 hash");

    let mocks = ProbeServerMockConfig::operator_contract(&sample_coin_id);
    let mut server = mockito::Server::new_async().await;
    let puzzle_all = mount_probe_server(&mut server, &p2_hex, &mocks)
        .await
        .expect("puzzle mock");

    let report = build_coinset_probe_report(probe_args(
        server.url(),
        launcher.clone(),
        mocks.peak_height,
    ))
    .await
    .expect("probe report");

    puzzle_all.assert_async().await;
    assert_operator_probe_report_contract(&report, &mocks, &launcher, &expected_p2_hash);
}

#[tokio::test]
async fn build_coinset_probe_report_soft_fails_unsupported_range_calls() {
    let launcher = launcher_id();
    let p2_hex = p2_coinset_hex(&launcher, 0);

    let mocks = ProbeServerMockConfig::empty_scan(100)
        .nested_peak()
        .with_puzzle_range(HttpMockBody::with_status(
            400,
            r#"{"success":false,"error":"range unsupported"}"#,
        ));
    let mut server = mockito::Server::new_async().await;
    let _ = mount_probe_server(&mut server, &p2_hex, &mocks).await;

    let report = build_coinset_probe_report(probe_args(server.url(), launcher, mocks.peak_height))
        .await
        .expect("probe report");

    assert!(
        report
            .capabilities
            .get_coin_records_by_puzzle_hashes
            .all_supported
    );
    assert!(
        !report
            .capabilities
            .get_coin_records_by_puzzle_hashes
            .range_supported
    );
    assert!(report
        .capabilities
        .get_coin_records_by_puzzle_hashes
        .range_error
        .as_deref()
        .unwrap_or("")
        .contains("range unsupported"));
    assert!(report
        .capabilities
        .get_coin_records_by_names
        .sample_name
        .is_none());
}

#[tokio::test]
async fn build_coinset_probe_report_skips_names_when_puzzle_scan_returns_no_sample() {
    let launcher = launcher_id();
    let p2_hex = p2_coinset_hex(&launcher, 0);

    let mocks = ProbeServerMockConfig::empty_scan(10);
    let mut server = mockito::Server::new_async().await;
    let _ = mount_probe_server(&mut server, &p2_hex, &mocks).await;

    let report = build_coinset_probe_report(probe_args(server.url(), launcher, mocks.peak_height))
        .await
        .expect("probe report");

    assert!(report
        .capabilities
        .get_coin_records_by_names
        .sample_name
        .is_none());
    assert!(report
        .capabilities
        .get_coin_records_by_names
        .all_supported
        .is_none());
}

#[test]
fn coinset_probe_report_serializes_top_level_contract_fields() {
    let report = greenfloor_engine::coinset_probe::ProbeReport {
        network: "mainnet".to_string(),
        coinset_base_url: "https://api.coinset.org".to_string(),
        launcher_id: "aa".repeat(32),
        launcher_id_source: "arg".to_string(),
        probe_nonce: 0,
        probe_p2_hash: "bb".repeat(32),
        scan_window: greenfloor_engine::coinset_probe::ScanWindow {
            start_height: 1,
            end_height: 2,
            peak_height: 2,
        },
        capabilities: greenfloor_engine::coinset_probe::CapabilitiesReport {
            get_coin_records_by_puzzle_hashes:
                greenfloor_engine::coinset_probe::EndpointCapability {
                    all_supported: true,
                    all_error: None,
                    all_count: Some(0),
                    range_supported: true,
                    range_error: None,
                    range_count: Some(0),
                },
            get_coin_records_by_hints: greenfloor_engine::coinset_probe::EndpointCapability {
                all_supported: true,
                all_error: None,
                all_count: Some(0),
                range_supported: true,
                range_error: None,
                range_count: Some(0),
            },
            get_coin_records_by_names: greenfloor_engine::coinset_probe::NamesCapability::skipped(),
        },
    };
    let payload = serde_json::to_value(report).expect("json");
    for key in [
        "network",
        "coinset_base_url",
        "launcher_id",
        "launcher_id_source",
        "probe_nonce",
        "probe_p2_hash",
        "scan_window",
        "capabilities",
    ] {
        assert!(payload.get(key).is_some(), "missing key {key}");
    }
}
