//! Mocked Coinset integration tests for `greenfloor-engine coinset probe`.

use greenfloor_engine::coinset::to_coinset_hex;
use greenfloor_engine::coinset_probe::{build_coinset_probe_report, CoinsetProbeCliArgs};
use greenfloor_engine::hex::normalize_hex_id;
use greenfloor_engine::vault::members::{
    hex_to_bytes32, singleton_member_puzzle_hash_hex_from_launcher_id,
};
use mockito::Matcher;
use serde_json::json;

const EMPTY_COIN_RECORDS: &str = r#"{"success":true,"coin_records":[]}"#;

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

#[derive(Clone)]
struct HttpMockBody {
    status: usize,
    body: String,
}

impl HttpMockBody {
    fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
        }
    }

    fn with_status(status: usize, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

struct ProbeScanMockConfig {
    peak_height: u64,
    puzzle_all: HttpMockBody,
    puzzle_all_expect: Option<usize>,
    puzzle_range: HttpMockBody,
    hints_all: HttpMockBody,
    hints_range: HttpMockBody,
    names_sample_coin_id: Option<String>,
}

async fn mount_blockchain_state(
    server: &mut mockito::ServerGuard,
    peak_height: u64,
    nested_peak: bool,
) {
    let body = if nested_peak {
        format!(r#"{{"success":true,"blockchain_state":{{"peak":{{"height":{peak_height}}}}}}}"#)
    } else {
        format!(r#"{{"success":true,"blockchain_state":{{"peak_height":{peak_height}}}}}"#)
    };
    let _state = server
        .mock("POST", "/get_blockchain_state")
        .with_status(200)
        .with_body(body)
        .create_async()
        .await;
}

struct CoinRecordsEndpointMock {
    endpoint: &'static str,
    payload_key: &'static str,
    payload_value: String,
    all: HttpMockBody,
    all_expect: Option<usize>,
    range: HttpMockBody,
}

async fn mount_coin_records_mocks(
    server: &mut mockito::ServerGuard,
    end_height: u64,
    mock: CoinRecordsEndpointMock,
) -> mockito::Mock {
    let CoinRecordsEndpointMock {
        endpoint,
        payload_key,
        payload_value,
        all,
        all_expect,
        range,
    } = mock;
    let all_body = json!({
        payload_key: [payload_value.clone()],
        "include_spent_coins": true,
    });
    let mut all_mock = server
        .mock("POST", endpoint)
        .match_body(Matcher::Json(all_body))
        .with_status(all.status)
        .with_body(all.body.as_str());
    if let Some(calls) = all_expect {
        all_mock = all_mock.expect(calls);
    }
    let all_mock = all_mock.create_async().await;

    let range_body = json!({
        payload_key: [payload_value],
        "include_spent_coins": true,
        "start_height": 0,
        "end_height": end_height,
    });
    let _range_mock = server
        .mock("POST", endpoint)
        .match_body(Matcher::Json(range_body))
        .with_status(range.status)
        .with_body(range.body.as_str())
        .create_async()
        .await;

    all_mock
}

async fn mount_puzzle_and_hints_mocks(
    server: &mut mockito::ServerGuard,
    p2_hex: &str,
    config: &ProbeScanMockConfig,
) -> Option<mockito::Mock> {
    let puzzle_all = mount_coin_records_mocks(
        server,
        config.peak_height,
        CoinRecordsEndpointMock {
            endpoint: "/get_coin_records_by_puzzle_hashes",
            payload_key: "puzzle_hashes",
            payload_value: p2_hex.to_string(),
            all: config.puzzle_all.clone(),
            all_expect: config.puzzle_all_expect,
            range: config.puzzle_range.clone(),
        },
    )
    .await;
    let _hints_all = mount_coin_records_mocks(
        server,
        config.peak_height,
        CoinRecordsEndpointMock {
            endpoint: "/get_coin_records_by_hints",
            payload_key: "hints",
            payload_value: p2_hex.to_string(),
            all: config.hints_all.clone(),
            all_expect: None,
            range: config.hints_range.clone(),
        },
    )
    .await;
    if let Some(sample_coin_id) = &config.names_sample_coin_id {
        let _names_all = mount_coin_records_mocks(
            server,
            config.peak_height,
            CoinRecordsEndpointMock {
                endpoint: "/get_coin_records_by_names",
                payload_key: "names",
                payload_value: format!("0x{sample_coin_id}"),
                all: HttpMockBody::ok(EMPTY_COIN_RECORDS),
                all_expect: None,
                range: HttpMockBody::ok(EMPTY_COIN_RECORDS),
            },
        )
        .await;
    }
    config.puzzle_all_expect.map(|_| puzzle_all)
}

fn puzzle_all_with_sample(sample_coin_id: &str) -> HttpMockBody {
    HttpMockBody::ok(format!(
        r#"{{"success":true,"coin_records":[{{"coin":{{"parent_coin_info":"0x{}","puzzle_hash":"0x{}","amount":1,"name":"0x{}"}}}}]}}"#,
        "aa".repeat(64),
        "bb".repeat(64),
        sample_coin_id
    ))
}

fn assert_operator_probe_report_contract(
    report: &greenfloor_engine::coinset_probe::ProbeReport,
    launcher: &str,
    expected_p2_hash: &str,
    sample_coin_id: &str,
) {
    assert_eq!(report.network, "mainnet");
    assert_eq!(report.launcher_id, launcher);
    assert_eq!(report.launcher_id_source, "arg");
    assert_eq!(report.probe_nonce, 0);
    assert_eq!(report.probe_p2_hash, expected_p2_hash);
    assert_eq!(report.scan_window.peak_height, 50_000);
    assert_eq!(report.scan_window.end_height, 50_000);
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

    let mut server = mockito::Server::new_async().await;
    mount_blockchain_state(&mut server, 50_000, false).await;
    let mocks = ProbeScanMockConfig {
        peak_height: 50_000,
        puzzle_all: puzzle_all_with_sample(&sample_coin_id),
        puzzle_all_expect: Some(1),
        puzzle_range: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        hints_all: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        hints_range: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        names_sample_coin_id: Some(sample_coin_id.clone()),
    };
    let puzzle_all = mount_puzzle_and_hints_mocks(&mut server, &p2_hex, &mocks)
        .await
        .expect("puzzle mock");

    let report = build_coinset_probe_report(probe_args(server.url(), launcher.clone(), 50_000))
        .await
        .expect("probe report");

    puzzle_all.assert_async().await;
    assert_operator_probe_report_contract(&report, &launcher, &expected_p2_hash, &sample_coin_id);
}

#[tokio::test]
async fn build_coinset_probe_report_soft_fails_unsupported_range_calls() {
    let launcher = launcher_id();
    let p2_hex = p2_coinset_hex(&launcher, 0);

    let mut server = mockito::Server::new_async().await;
    mount_blockchain_state(&mut server, 100, true).await;
    let mocks = ProbeScanMockConfig {
        peak_height: 100,
        puzzle_all: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        puzzle_all_expect: None,
        puzzle_range: HttpMockBody::with_status(
            400,
            r#"{"success":false,"error":"range unsupported"}"#,
        ),
        hints_all: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        hints_range: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        names_sample_coin_id: None,
    };
    let _ = mount_puzzle_and_hints_mocks(&mut server, &p2_hex, &mocks).await;

    let report = build_coinset_probe_report(probe_args(server.url(), launcher, 100))
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

    let mut server = mockito::Server::new_async().await;
    mount_blockchain_state(&mut server, 10, false).await;
    let mocks = ProbeScanMockConfig {
        peak_height: 10,
        puzzle_all: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        puzzle_all_expect: None,
        puzzle_range: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        hints_all: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        hints_range: HttpMockBody::ok(EMPTY_COIN_RECORDS),
        names_sample_coin_id: None,
    };
    let _ = mount_puzzle_and_hints_mocks(&mut server, &p2_hex, &mocks).await;

    let report = build_coinset_probe_report(probe_args(server.url(), launcher, 10))
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
