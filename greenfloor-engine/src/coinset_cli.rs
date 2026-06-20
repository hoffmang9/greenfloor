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
mod tests;
