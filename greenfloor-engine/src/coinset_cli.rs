use clap::{Args, Subcommand};
use serde_json::{json, Value};

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::coinset::{
    get_conservative_fee_estimate, get_fee_estimate, post_coinset_rpc, push_tx_hex,
};
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
pub struct CoinsetFeeEstimateArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub base_url: String,
    #[arg(long, value_delimiter = ',', default_value = "60,300,600")]
    pub target_times: Vec<u64>,
    #[arg(long, default_value_t = 1_000_000)]
    pub cost: u64,
    #[arg(long)]
    pub spend_count: Option<u64>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetConservativeFeeEstimateArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub base_url: String,
    #[arg(long, default_value_t = 1_000_000)]
    pub cost: u64,
    #[arg(long)]
    pub spend_count: Option<u64>,
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
    #[command(name = "fee-estimate")]
    FeeEstimate(CoinsetFeeEstimateArgs),
    #[command(name = "conservative-fee-estimate")]
    ConservativeFeeEstimate(CoinsetConservativeFeeEstimateArgs),
}

#[derive(Debug, Args)]
pub struct CoinsetCliArgs {
    #[command(subcommand)]
    pub command: CoinsetCommands,
}

pub fn conservative_fee_json_value(fee: Option<u64>) -> Value {
    json!({ "fee_mojos": fee })
}

pub async fn run_coinset_command(args: CoinsetCliArgs) -> SignerResult<()> {
    match args.command {
        CoinsetCommands::Post(args) => run_coinset_post(args).await,
        CoinsetCommands::PushTx(args) => run_coinset_push_tx(args).await,
        CoinsetCommands::FeeEstimate(args) => run_coinset_fee_estimate(args).await,
        CoinsetCommands::ConservativeFeeEstimate(args) => {
            run_coinset_conservative_fee_estimate(args).await
        }
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

pub async fn run_coinset_fee_estimate(args: CoinsetFeeEstimateArgs) -> SignerResult<()> {
    let payload = get_fee_estimate(
        &args.network,
        optional_trimmed(&args.base_url).as_deref(),
        args.target_times,
        args.cost,
        args.spend_count,
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

pub async fn run_coinset_conservative_fee_estimate(
    args: CoinsetConservativeFeeEstimateArgs,
) -> SignerResult<()> {
    let fee = get_conservative_fee_estimate(
        &args.network,
        optional_trimmed(&args.base_url).as_deref(),
        args.cost,
        args.spend_count,
    )
    .await?;
    if args.json {
        print_json_value(&conservative_fee_json_value(fee), true)?;
    } else if let Some(value) = fee {
        println!("{value}");
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
            other => panic!("unexpected subcommand: {other:?}"),
        }
    }

    #[test]
    fn parses_nested_coinset_fee_estimate_with_json() {
        let cli = TestCli::try_parse_from([
            "test",
            "fee-estimate",
            "--network",
            "testnet11",
            "--base-url",
            "https://testnet11.api.coinset.org",
            "--cost",
            "2000000",
            "--json",
        ])
        .expect("parse coinset fee-estimate");
        match cli.command {
            CoinsetCommands::FeeEstimate(args) => {
                assert_eq!(args.network, "testnet11");
                assert_eq!(args.base_url, "https://testnet11.api.coinset.org");
                assert_eq!(args.cost, 2_000_000);
                assert!(args.json);
            }
            other => panic!("unexpected subcommand: {other:?}"),
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
            other => panic!("unexpected subcommand: {other:?}"),
        }
    }

    #[test]
    fn conservative_fee_json_envelope_shape() {
        let value = conservative_fee_json_value(Some(500));
        assert_eq!(value.get("fee_mojos").and_then(Value::as_u64), Some(500));
        let none_value = conservative_fee_json_value(None);
        assert!(none_value.get("fee_mojos").unwrap().is_null());
    }
}
