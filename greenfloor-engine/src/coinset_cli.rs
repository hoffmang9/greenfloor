use clap::{Args, Subcommand};
use serde_json::Value;

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::coinset::{post_coinset_rpc, push_tx_hex};
use crate::error::SignerResult;

#[derive(Debug, Args)]
pub struct CoinsetPushTxArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub base_url: String,
    #[arg(long)]
    pub spend_bundle_hex: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetPostArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub base_url: String,
    #[arg(long)]
    pub endpoint: String,
    #[arg(long, default_value = "{}")]
    pub body_json: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum CoinsetCommands {
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

pub async fn run_coinset_command(args: CoinsetCliArgs) -> SignerResult<()> {
    match args.command {
        CoinsetCommands::Post(args) => run_coinset_post(args).await,
        CoinsetCommands::PushTx(args) => run_coinset_push_tx(args).await,
    }
}

pub async fn run_coinset_post(args: CoinsetPostArgs) -> SignerResult<()> {
    let body: Value = serde_json::from_str(&args.body_json)
        .map_err(|err| crate::error::SignerError::Other(format!("parse body json: {err}")))?;
    let payload = post_coinset_rpc(
        &args.network,
        optional_trimmed(&args.base_url).as_deref(),
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
        &args.network,
        optional_trimmed(&args.base_url).as_deref(),
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
            other @ CoinsetCommands::PushTx(_) => panic!("unexpected subcommand: {other:?}"),
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
            other @ CoinsetCommands::Post(_) => panic!("unexpected subcommand: {other:?}"),
        }
    }
}
