use std::io::{self, Read};

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::coinset::{
    coin_id_from_record, ensure_coinset_rpc_success, post_coinset_coin_records,
    post_coinset_record, post_coinset_rpc, push_tx_hex, resolve_direct_client,
};
use crate::coinset_probe::run_coinset_probe_command;
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Args)]
struct CoinsetClientArgs {
    #[arg(long, default_value = "mainnet")]
    network: String,
    #[arg(long, default_value = "")]
    base_url: String,
}

#[derive(Debug, Args)]
pub struct CoinsetPushTxArgs {
    #[command(flatten)]
    client: CoinsetClientArgs,
    #[arg(long)]
    pub spend_bundle_hex: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetPostArgs {
    #[command(flatten)]
    client: CoinsetClientArgs,
    #[arg(long)]
    pub endpoint: String,
    #[arg(long, default_value = "{}")]
    pub body_json: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetResolveClientArgs {
    #[command(flatten)]
    client: CoinsetClientArgs,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetCoinRecordsArgs {
    #[command(flatten)]
    client: CoinsetClientArgs,
    #[arg(long)]
    pub endpoint: String,
    #[arg(long, default_value = "{}")]
    pub body_json: String,
    #[arg(long)]
    pub start_height: Option<u64>,
    #[arg(long)]
    pub end_height: Option<u64>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetCoinIdFromRecordArgs {
    #[arg(long, default_value = "")]
    pub record_json: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetRecordArgs {
    #[command(flatten)]
    client: CoinsetClientArgs,
    #[arg(long)]
    pub endpoint: String,
    #[arg(long, default_value = "{}")]
    pub body_json: String,
    #[arg(long)]
    pub key: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum CoinsetCommands {
    #[command(name = "resolve-client")]
    ResolveClient(CoinsetResolveClientArgs),
    #[command(name = "coin-records")]
    CoinRecords(CoinsetCoinRecordsArgs),
    #[command(name = "record")]
    Record(CoinsetRecordArgs),
    #[command(name = "post")]
    Post(CoinsetPostArgs),
    #[command(name = "coin-id-from-record")]
    CoinIdFromRecord(CoinsetCoinIdFromRecordArgs),
    #[command(name = "push-tx")]
    PushTx(CoinsetPushTxArgs),
    /// Probe Coinset height-window API support for vault scans.
    Probe(crate::coinset_probe::CoinsetProbeCliArgs),
}

#[derive(Debug, Args)]
pub struct CoinsetCliArgs {
    #[command(subcommand)]
    pub command: CoinsetCommands,
}

fn client_base_url(base_url: &str) -> Option<String> {
    optional_trimmed(base_url)
}

fn parse_body_json(body_json: &str) -> SignerResult<Value> {
    serde_json::from_str(body_json)
        .map_err(|err| SignerError::Other(format!("parse body json: {err}")))
}

fn apply_height_fields(body: &mut Value, start_height: Option<u64>, end_height: Option<u64>) {
    if let Some(obj) = body.as_object_mut() {
        if let Some(start_height) = start_height {
            obj.insert("start_height".to_string(), json!(start_height));
        }
        if let Some(end_height) = end_height {
            obj.insert("end_height".to_string(), json!(end_height));
        }
    }
}

fn emit_json_or(payload: &Value, json: bool, human: impl FnOnce()) -> SignerResult<()> {
    if json {
        print_json_value(payload, true)
    } else {
        human();
        Ok(())
    }
}

/// Run coinset command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_coinset_command(args: CoinsetCliArgs) -> SignerResult<()> {
    match args.command {
        CoinsetCommands::ResolveClient(args) => run_coinset_resolve_client(&args),
        CoinsetCommands::CoinRecords(args) => run_coinset_coin_records(args).await,
        CoinsetCommands::Record(args) => run_coinset_record(args).await,
        CoinsetCommands::Post(args) => run_coinset_post(args).await,
        CoinsetCommands::CoinIdFromRecord(args) => run_coinset_coin_id_from_record(&args),
        CoinsetCommands::PushTx(args) => run_coinset_push_tx(args).await,
        CoinsetCommands::Probe(args) => run_coinset_probe_command(args).await,
    }
}

/// Run coinset resolve client.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn run_coinset_resolve_client(args: &CoinsetResolveClientArgs) -> SignerResult<()> {
    let resolved = resolve_direct_client(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
    );
    let payload = json!({
        "network": resolved.network,
        "base_url": resolved.base_url,
    });
    emit_json_or(&payload, args.json, || {
        println!("network: {}", resolved.network);
        println!("base_url: {}", resolved.base_url);
    })
}

/// Run coinset coin records.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_coinset_coin_records(args: CoinsetCoinRecordsArgs) -> SignerResult<()> {
    let mut body = parse_body_json(&args.body_json)?;
    apply_height_fields(&mut body, args.start_height, args.end_height);
    let records = post_coinset_coin_records(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.endpoint,
        body,
    )
    .await?;
    let count = records.len();
    let payload = json!({ "coin_records": records });
    emit_json_or(&payload, args.json, || println!("coin_records: {count}"))
}

/// Run coinset record.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_coinset_record(args: CoinsetRecordArgs) -> SignerResult<()> {
    let body = parse_body_json(&args.body_json)?;
    let record = post_coinset_record(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.endpoint,
        body,
        &args.key,
    )
    .await?;
    let present = record.is_some();
    let payload = json!({ "record": record });
    emit_json_or(&payload, args.json, || {
        println!("record_present: {present}");
    })
}

/// Run coinset post.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_coinset_post(args: CoinsetPostArgs) -> SignerResult<()> {
    let body = parse_body_json(&args.body_json)?;
    let payload = post_coinset_rpc(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.endpoint,
        body,
    )
    .await?;
    ensure_coinset_rpc_success(&payload)?;
    emit_json_or(&payload, args.json, || println!("success: true"))
}

fn read_record_json(record_json: &str) -> SignerResult<Value> {
    let trimmed = record_json.trim();
    if !trimmed.is_empty() {
        return parse_body_json(trimmed);
    }
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|err| SignerError::Other(format!("read record json from stdin: {err}")))?;
    parse_body_json(&buffer)
}

/// Run coinset coin id from record.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn run_coinset_coin_id_from_record(args: &CoinsetCoinIdFromRecordArgs) -> SignerResult<()> {
    let record = read_record_json(&args.record_json)?;
    let coin_id = coin_id_from_record(&record);
    let payload = json!({ "coin_id": coin_id });
    emit_json_or(&payload, args.json, || println!("{coin_id}"))
}

/// Run coinset push tx.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_coinset_push_tx(args: CoinsetPushTxArgs) -> SignerResult<()> {
    let payload = push_tx_hex(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.spend_bundle_hex,
    )
    .await?;
    let success = payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let status = payload.get("status").and_then(Value::as_str);
    emit_json_or(&payload, args.json, || {
        println!("success: {success}");
        if let Some(status) = status {
            println!("status: {status}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_util::script_engine_error_retryable;
    use crate::coinset::{
        coin_id_from_record, ensure_coinset_rpc_success, post_coinset_coin_records,
        post_coinset_record, post_coinset_rpc, push_tx_hex, resolve_direct_client,
        TESTNET11_DIRECT_BASE_URL,
    };
    use crate::error::SignerError;
    use chia_protocol::SpendBundle;
    use chia_protocol::{Bytes32, Coin};
    use chia_traits::Streamable;
    use clap::Parser;
    use mockito::Matcher;
    use serde_json::json;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: CoinsetCommands,
    }

    #[test]
    fn parses_nested_coinset_post_with_json() {
        let cli = TestCli::try_parse_from([
            "test",
            "post",
            "--endpoint",
            "get_all_mempool_tx_ids",
            "--body-json",
            "{}",
            "--json",
        ])
        .expect("parse coinset post");
        match cli.command {
            CoinsetCommands::Post(args) => {
                assert_eq!(args.endpoint, "get_all_mempool_tx_ids");
                assert_eq!(args.body_json, "{}");
                assert!(args.json);
            }
            _ => panic!("unexpected subcommand"),
        }
    }

    #[test]
    fn parses_nested_coinset_push_tx() {
        let cli = TestCli::try_parse_from([
            "test",
            "push-tx",
            "--spend-bundle-hex",
            "deadbeef",
            "--json",
        ])
        .expect("parse coinset push-tx");
        match cli.command {
            CoinsetCommands::PushTx(args) => {
                assert_eq!(args.spend_bundle_hex, "deadbeef");
                assert!(args.json);
            }
            _ => panic!("unexpected subcommand"),
        }
    }

    #[test]
    fn parses_coinset_coin_id_from_record() {
        let cli = TestCli::try_parse_from([
            "test",
            "coin-id-from-record",
            "--record-json",
            r#"{"coin":{"amount":1}}"#,
            "--json",
        ])
        .expect("parse coinset coin-id-from-record");
        match cli.command {
            CoinsetCommands::CoinIdFromRecord(args) => {
                assert_eq!(args.record_json, r#"{"coin":{"amount":1}}"#);
                assert!(args.json);
            }
            _ => panic!("unexpected subcommand"),
        }
    }

    #[test]
    fn parses_coinset_coin_records_with_heights() {
        let cli = TestCli::try_parse_from([
            "test",
            "coin-records",
            "--endpoint",
            "get_coin_records_by_puzzle_hash",
            "--body-json",
            r#"{"puzzle_hash":"0x01"}"#,
            "--start-height",
            "10",
            "--end-height",
            "20",
            "--json",
        ])
        .expect("parse coinset coin-records");
        match cli.command {
            CoinsetCommands::CoinRecords(args) => {
                assert_eq!(args.endpoint, "get_coin_records_by_puzzle_hash");
                assert_eq!(args.start_height, Some(10));
                assert_eq!(args.end_height, Some(20));
                assert!(args.json);
            }
            _ => panic!("unexpected subcommand"),
        }
    }

    #[test]
    fn parses_nested_coinset_probe_defaults() {
        let cli = TestCli::try_parse_from([
            "test",
            "probe",
            "--launcher-id",
            &"ab".repeat(32),
            "--height-window",
            "1000",
        ])
        .expect("parse coinset probe");
        match cli.command {
            CoinsetCommands::Probe(args) => {
                assert_eq!(args.height_window, 1000);
                assert_eq!(args.launcher_id.len(), 64);
            }
            _ => panic!("unexpected subcommand"),
        }
    }

    #[test]
    fn resolve_client_testnet_defaults_without_base_url() {
        let resolved = resolve_direct_client("testnet", None);
        assert_eq!(resolved.network, "testnet11");
        assert_eq!(resolved.base_url, TESTNET11_DIRECT_BASE_URL);
    }

    #[tokio::test]
    async fn coin_records_testnet_without_base_url() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_blockchain_state")
            .with_status(200)
            .with_body(r#"{"success":true,"blockchain_state":{"peak_height":42}}"#)
            .create_async()
            .await;

        let state = post_coinset_record(
            "testnet",
            Some(&server.url()),
            "get_blockchain_state",
            json!({}),
            "blockchain_state",
        )
        .await
        .expect("record")
        .expect("some state");
        assert_eq!(
            state.get("peak_height").and_then(serde_json::Value::as_i64),
            Some(42)
        );
    }

    #[tokio::test]
    async fn coin_records_filters_non_objects_and_height_flags() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .match_body(Matcher::PartialJson(json!({
                "puzzle_hash": "0x11",
                "include_spent_coins": false,
                "start_height": 10,
                "end_height": 20,
            })))
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[{"coin":{"amount":1}},"bad"]}"#)
            .create_async()
            .await;

        let records = post_coinset_coin_records(
            "mainnet",
            Some(&server.url()),
            "get_coin_records_by_puzzle_hash",
            json!({
                "puzzle_hash": "0x11",
                "include_spent_coins": false,
                "start_height": 10,
                "end_height": 20,
            }),
        )
        .await
        .expect("coin records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["coin"]["amount"], 1);
    }

    #[tokio::test]
    async fn coin_records_fails_on_success_false() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(r#"{"success":false,"error":"invalid puzzle hash"}"#)
            .create_async()
            .await;

        let err = post_coinset_coin_records(
            "mainnet",
            Some(&server.url()),
            "get_coin_records_by_puzzle_hash",
            json!({"puzzle_hash":"0x11","include_spent_coins":false}),
        )
        .await
        .expect_err("success=false");
        assert_eq!(err.to_string(), "coinset error: invalid puzzle hash");
        assert!(!script_engine_error_retryable(&err));
    }

    #[tokio::test]
    async fn coin_records_surfaces_coinset_error_on_http_503() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(503)
            .with_body("service unavailable")
            .create_async()
            .await;

        let err = post_coinset_coin_records(
            "mainnet",
            Some(&server.url()),
            "get_coin_records_by_puzzle_hash",
            json!({"puzzle_hash":"0x11","include_spent_coins":false}),
        )
        .await
        .expect_err("503");
        assert!(script_engine_error_retryable(&err));
    }

    #[tokio::test]
    async fn coin_records_connection_refused_is_retryable() {
        let err = post_coinset_coin_records(
            "mainnet",
            Some("http://127.0.0.1:1"),
            "get_coin_records_by_puzzle_hash",
            json!({"puzzle_hash":"0x11","include_spent_coins":false}),
        )
        .await
        .expect_err("connection refused");
        assert!(matches!(err, SignerError::Coinset(_)));
        assert!(script_engine_error_retryable(&err));
    }

    #[tokio::test]
    async fn post_fee_estimate_returns_rpc_payload() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(200)
            .with_body(r#"{"success":true,"estimates":[100,500,200]}"#)
            .create_async()
            .await;

        let value = post_coinset_rpc(
            "mainnet",
            Some(&server.url()),
            "get_fee_estimate",
            json!({"target_times":[300,600,1200],"cost":1000000}),
        )
        .await
        .expect("fee estimate");
        assert_eq!(
            value.get("success").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        let estimates = value
            .get("estimates")
            .and_then(serde_json::Value::as_array)
            .expect("estimates");
        assert_eq!(estimates.len(), 3);
    }

    #[tokio::test]
    async fn post_returns_rpc_payload() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"mempool_tx_ids":["0xabc","0xdef"]}"#)
            .create_async()
            .await;

        let value = post_coinset_rpc(
            "mainnet",
            Some(&server.url()),
            "get_all_mempool_tx_ids",
            json!({}),
        )
        .await
        .expect("post");
        let tx_ids = value
            .get("mempool_tx_ids")
            .and_then(serde_json::Value::as_array)
            .expect("mempool_tx_ids");
        assert_eq!(tx_ids.len(), 2);
    }

    #[tokio::test]
    async fn post_fails_on_success_false() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":false,"error":"mempool unavailable"}"#)
            .create_async()
            .await;

        let payload = post_coinset_rpc(
            "mainnet",
            Some(&server.url()),
            "get_all_mempool_tx_ids",
            json!({}),
        )
        .await
        .expect("rpc payload");
        let err = ensure_coinset_rpc_success(&payload).expect_err("success=false");
        assert_eq!(err.to_string(), "coinset error: mempool unavailable");
    }

    #[test]
    fn coin_id_from_record_computes_coin_id() {
        let parent = Bytes32::new([0x11; 32]);
        let puzzle_hash = Bytes32::new([0x22; 32]);
        let amount = 42_u64;
        let expected = hex::encode(Coin::new(parent, puzzle_hash, amount).coin_id());
        let record = json!({
            "coin": {
                "parent_coin_info": format!("0x{}", hex::encode(parent)),
                "puzzle_hash": format!("0x{}", hex::encode(puzzle_hash)),
                "amount": amount,
            }
        });
        assert_eq!(coin_id_from_record(&record), expected);
    }

    #[tokio::test]
    async fn push_tx_emits_success_payload() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/push_tx")
            .with_status(200)
            .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
            .create_async()
            .await;

        let bundle = SpendBundle::new(Vec::new(), chia_bls::Signature::default());
        let spend_bundle_hex = hex::encode(bundle.to_bytes().expect("serialize bundle"));
        let value = push_tx_hex("mainnet", Some(&server.url()), &spend_bundle_hex)
            .await
            .expect("push tx");
        assert_eq!(
            value.get("success").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value.get("status").and_then(serde_json::Value::as_str),
            Some("SUCCESS")
        );
    }
}
