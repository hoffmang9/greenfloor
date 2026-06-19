use clap::{Args, Subcommand};
use serde_json::{json, Value};

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::coinset::{
    post_coinset_coin_records, post_coinset_record, post_coinset_rpc, push_tx_hex,
    resolve_direct_client,
};
use crate::error::SignerResult;

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
    #[command(name = "push-tx")]
    PushTx(CoinsetPushTxArgs),
}

#[derive(Debug, Args)]
pub struct CoinsetCliArgs {
    #[command(subcommand)]
    pub command: CoinsetCommands,
}

fn client_base_url(base_url: &str) -> Option<String> {
    optional_trimmed(base_url)
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

pub async fn run_coinset_command(args: CoinsetCliArgs) -> SignerResult<()> {
    match args.command {
        CoinsetCommands::ResolveClient(args) => run_coinset_resolve_client(&args),
        CoinsetCommands::CoinRecords(args) => run_coinset_coin_records(args).await,
        CoinsetCommands::Record(args) => run_coinset_record(args).await,
        CoinsetCommands::Post(args) => run_coinset_post(args).await,
        CoinsetCommands::PushTx(args) => run_coinset_push_tx(args).await,
    }
}

pub fn run_coinset_resolve_client(args: &CoinsetResolveClientArgs) -> SignerResult<()> {
    let resolved = resolve_direct_client(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
    );
    let payload = json!({
        "network": resolved.network,
        "base_url": resolved.base_url,
    });
    if args.json {
        print_json_value(&payload, true)?;
    } else {
        println!("network: {}", resolved.network);
        println!("base_url: {}", resolved.base_url);
    }
    Ok(())
}

pub async fn run_coinset_coin_records(args: CoinsetCoinRecordsArgs) -> SignerResult<()> {
    let mut body: Value = serde_json::from_str(&args.body_json)
        .map_err(|err| crate::error::SignerError::Other(format!("parse body json: {err}")))?;
    apply_height_fields(&mut body, args.start_height, args.end_height);
    let records = post_coinset_coin_records(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.endpoint,
        body,
    )
    .await?;
    let payload = json!({ "coin_records": records });
    if args.json {
        print_json_value(&payload, true)?;
    } else {
        println!("coin_records: {}", records.len());
    }
    Ok(())
}

pub async fn run_coinset_record(args: CoinsetRecordArgs) -> SignerResult<()> {
    let body: Value = serde_json::from_str(&args.body_json)
        .map_err(|err| crate::error::SignerError::Other(format!("parse body json: {err}")))?;
    let record = post_coinset_record(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.endpoint,
        body,
        &args.key,
    )
    .await?;
    let payload = json!({ "record": record });
    if args.json {
        print_json_value(&payload, true)?;
    } else {
        println!("record_present: {}", record.is_some());
    }
    Ok(())
}

pub async fn run_coinset_post(args: CoinsetPostArgs) -> SignerResult<()> {
    let body: Value = serde_json::from_str(&args.body_json)
        .map_err(|err| crate::error::SignerError::Other(format!("parse body json: {err}")))?;
    let payload = post_coinset_rpc(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.endpoint,
        body,
    )
    .await?;
    if args.json {
        print_json_value(&payload, true)?;
    } else {
        println!(
            "success: {}",
            payload
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
    }
    Ok(())
}

pub async fn run_coinset_push_tx(args: CoinsetPushTxArgs) -> SignerResult<()> {
    let payload = push_tx_hex(
        &args.client.network,
        client_base_url(&args.client.base_url).as_deref(),
        &args.spend_bundle_hex,
    )
    .await?;
    if args.json {
        print_json_value(&payload, true)?;
    } else {
        println!(
            "success: {}",
            payload
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        );
        if let Some(status) = payload.get("status").and_then(Value::as_str) {
            println!("status: {status}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

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
}
