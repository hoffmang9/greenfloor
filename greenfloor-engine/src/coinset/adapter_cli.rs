use clap::{Args, Subcommand};
use serde_json::{json, Value};

use crate::cli_util::{optional_trimmed, print_json_value};
use crate::coinset::CoinsetAdapter;
use crate::error::SignerResult;

#[derive(Debug, Args)]
pub struct CoinsetClientArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub base_url: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CoinsetHeightRangeArgs {
    #[arg(long)]
    pub start_height: Option<u64>,
    #[arg(long)]
    pub end_height: Option<u64>,
}

#[derive(Debug, Args)]
pub struct CoinsetGetMempoolTxIdsArgs {
    #[command(flatten)]
    pub client: CoinsetClientArgs,
}

#[derive(Debug, Args)]
pub struct CoinsetGetCoinRecordsByPuzzleHashArgs {
    #[command(flatten)]
    pub client: CoinsetClientArgs,
    #[arg(long)]
    pub puzzle_hash_hex: String,
    #[arg(long, default_value_t = false)]
    pub include_spent_coins: bool,
    #[command(flatten)]
    pub height_range: CoinsetHeightRangeArgs,
}

#[derive(Debug, Args)]
pub struct CoinsetGetCoinRecordsListArgs {
    #[command(flatten)]
    pub client: CoinsetClientArgs,
    #[arg(long, value_delimiter = ',')]
    pub values_hex: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub include_spent_coins: bool,
    #[command(flatten)]
    pub height_range: CoinsetHeightRangeArgs,
}

#[derive(Debug, Args)]
pub struct CoinsetGetCoinRecordByNameArgs {
    #[command(flatten)]
    pub client: CoinsetClientArgs,
    #[arg(long)]
    pub coin_name_hex: String,
}

#[derive(Debug, Args)]
pub struct CoinsetGetPuzzleAndSolutionArgs {
    #[command(flatten)]
    pub client: CoinsetClientArgs,
    #[arg(long)]
    pub coin_id_hex: String,
    #[arg(long)]
    pub height: Option<u64>,
}

#[derive(Debug, Args)]
pub struct CoinsetGetBlockchainStateArgs {
    #[command(flatten)]
    pub client: CoinsetClientArgs,
}

#[derive(Debug, Args)]
pub struct CoinsetGetCoinRecordsByHintArgs {
    #[command(flatten)]
    pub client: CoinsetClientArgs,
    #[arg(long)]
    pub hint_hex: String,
    #[arg(long, default_value_t = false)]
    pub include_spent_coins: bool,
    #[command(flatten)]
    pub height_range: CoinsetHeightRangeArgs,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Subcommand)]
pub enum CoinsetAdapterCommands {
    #[command(name = "get-mempool-tx-ids")]
    GetMempoolTxIds(CoinsetGetMempoolTxIdsArgs),
    #[command(name = "get-coin-records-by-puzzle-hash")]
    GetCoinRecordsByPuzzleHash(CoinsetGetCoinRecordsByPuzzleHashArgs),
    #[command(name = "get-coin-records-by-puzzle-hashes")]
    GetCoinRecordsByPuzzleHashes(CoinsetGetCoinRecordsListArgs),
    #[command(name = "get-coin-record-by-name")]
    GetCoinRecordByName(CoinsetGetCoinRecordByNameArgs),
    #[command(name = "get-coin-records-by-names")]
    GetCoinRecordsByNames(CoinsetGetCoinRecordsListArgs),
    #[command(name = "get-coin-records-by-parent-ids")]
    GetCoinRecordsByParentIds(CoinsetGetCoinRecordsListArgs),
    #[command(name = "get-coin-records-by-hint")]
    GetCoinRecordsByHint(CoinsetGetCoinRecordsByHintArgs),
    #[command(name = "get-coin-records-by-hints")]
    GetCoinRecordsByHints(CoinsetGetCoinRecordsListArgs),
    #[command(name = "get-puzzle-and-solution")]
    GetPuzzleAndSolution(CoinsetGetPuzzleAndSolutionArgs),
    #[command(name = "get-blockchain-state")]
    GetBlockchainState(CoinsetGetBlockchainStateArgs),
}

fn adapter_for_args(network: &str, base_url: &str) -> CoinsetAdapter {
    CoinsetAdapter::new(optional_trimmed(base_url).as_deref(), network)
}

fn print_adapter_json(args_json: bool, value: &Value) -> SignerResult<()> {
    if args_json {
        print_json_value(value, true)
    } else {
        println!(
            "{}",
            serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
        );
        Ok(())
    }
}

pub async fn run_coinset_adapter_command(command: CoinsetAdapterCommands) -> SignerResult<()> {
    match command {
        CoinsetAdapterCommands::GetMempoolTxIds(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let tx_ids = adapter.get_all_mempool_tx_ids().await?;
            let payload = json!({ "tx_ids": tx_ids });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetCoinRecordsByPuzzleHash(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let records = adapter
                .get_coin_records_by_puzzle_hash(
                    &args.puzzle_hash_hex,
                    args.include_spent_coins,
                    args.height_range.start_height,
                    args.height_range.end_height,
                )
                .await?;
            let payload = json!({ "coin_records": records });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetCoinRecordsByPuzzleHashes(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let records = adapter
                .get_coin_records_by_puzzle_hashes(
                    &args.values_hex,
                    args.include_spent_coins,
                    args.height_range.start_height,
                    args.height_range.end_height,
                )
                .await?;
            let payload = json!({ "coin_records": records });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetCoinRecordByName(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let record = adapter.get_coin_record_by_name(&args.coin_name_hex).await?;
            let payload = json!({ "coin_record": record });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetCoinRecordsByNames(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let records = adapter
                .get_coin_records_by_names(
                    &args.values_hex,
                    args.include_spent_coins,
                    args.height_range.start_height,
                    args.height_range.end_height,
                )
                .await?;
            let payload = json!({ "coin_records": records });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetCoinRecordsByParentIds(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let records = adapter
                .get_coin_records_by_parent_ids(
                    &args.values_hex,
                    args.include_spent_coins,
                    args.height_range.start_height,
                    args.height_range.end_height,
                )
                .await?;
            let payload = json!({ "coin_records": records });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetCoinRecordsByHint(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let records = adapter
                .get_coin_records_by_hint(
                    &args.hint_hex,
                    args.include_spent_coins,
                    args.height_range.start_height,
                    args.height_range.end_height,
                )
                .await?;
            let payload = json!({ "coin_records": records });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetCoinRecordsByHints(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let records = adapter
                .get_coin_records_by_hints(
                    &args.values_hex,
                    args.include_spent_coins,
                    args.height_range.start_height,
                    args.height_range.end_height,
                )
                .await?;
            let payload = json!({ "coin_records": records });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetPuzzleAndSolution(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let solution = adapter
                .get_puzzle_and_solution(&args.coin_id_hex, args.height)
                .await?;
            let payload = json!({ "coin_solution": solution });
            print_adapter_json(args.client.json, &payload)?;
        }
        CoinsetAdapterCommands::GetBlockchainState(args) => {
            let adapter = adapter_for_args(&args.client.network, &args.client.base_url);
            let state = adapter.get_blockchain_state().await?;
            let payload = json!({ "blockchain_state": state });
            print_adapter_json(args.client.json, &payload)?;
        }
    }
    Ok(())
}
