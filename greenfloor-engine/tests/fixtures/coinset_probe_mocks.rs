use mockito::Matcher;
use serde_json::json;

pub const EMPTY_COIN_RECORDS: &str = r#"{"success":true,"coin_records":[]}"#;

#[derive(Clone)]
pub struct HttpMockBody {
    pub status: usize,
    pub body: String,
}

impl HttpMockBody {
    pub fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
        }
    }

    pub fn with_status(status: usize, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamesMockMode {
    Skip,
    WithSample(String),
}

pub struct ProbeServerMockConfig {
    pub peak_height: u64,
    pub nested_peak: bool,
    pub puzzle_all: HttpMockBody,
    pub puzzle_all_expect: Option<usize>,
    pub puzzle_range: HttpMockBody,
    pub hints_all: HttpMockBody,
    pub hints_range: HttpMockBody,
    pub names: NamesMockMode,
}

impl ProbeServerMockConfig {
    fn empty_records(peak_height: u64) -> Self {
        let empty = HttpMockBody::ok(EMPTY_COIN_RECORDS);
        Self {
            peak_height,
            nested_peak: false,
            puzzle_all: empty.clone(),
            puzzle_all_expect: None,
            puzzle_range: empty.clone(),
            hints_all: empty.clone(),
            hints_range: empty,
            names: NamesMockMode::Skip,
        }
    }

    pub fn empty_scan(peak_height: u64) -> Self {
        Self::empty_records(peak_height)
    }

    pub fn operator_contract(sample_coin_id: impl Into<String>) -> Self {
        let sample_coin_id = sample_coin_id.into();
        let empty = HttpMockBody::ok(EMPTY_COIN_RECORDS);
        Self {
            peak_height: 50_000,
            nested_peak: false,
            puzzle_all: puzzle_all_with_sample(&sample_coin_id),
            puzzle_all_expect: Some(1),
            puzzle_range: empty.clone(),
            hints_all: empty.clone(),
            hints_range: empty,
            names: NamesMockMode::WithSample(sample_coin_id),
        }
    }

    pub fn nested_peak(mut self) -> Self {
        self.nested_peak = true;
        self
    }

    pub fn with_puzzle_range(mut self, range: HttpMockBody) -> Self {
        self.puzzle_range = range;
        self
    }
}

struct CoinRecordsEndpointMock {
    endpoint: &'static str,
    payload_key: &'static str,
    payload_value: String,
    all: HttpMockBody,
    all_expect: Option<usize>,
    range: HttpMockBody,
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

async fn mount_scan_endpoint_mocks(
    server: &mut mockito::ServerGuard,
    p2_hex: &str,
    config: &ProbeServerMockConfig,
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
    if let NamesMockMode::WithSample(sample_coin_id) = &config.names {
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

pub fn puzzle_all_with_sample(sample_coin_id: &str) -> HttpMockBody {
    HttpMockBody::ok(format!(
        r#"{{"success":true,"coin_records":[{{"coin":{{"parent_coin_info":"0x{}","puzzle_hash":"0x{}","amount":1,"name":"0x{}"}}}}]}}"#,
        "aa".repeat(64),
        "bb".repeat(64),
        sample_coin_id
    ))
}

pub async fn mount_probe_server(
    server: &mut mockito::ServerGuard,
    p2_hex: &str,
    config: &ProbeServerMockConfig,
) -> Option<mockito::Mock> {
    mount_blockchain_state(server, config.peak_height, config.nested_peak).await;
    mount_scan_endpoint_mocks(server, p2_hex, config).await
}
