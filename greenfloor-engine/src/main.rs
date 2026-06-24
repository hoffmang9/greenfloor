use std::path::PathBuf;

use clap::{Parser, Subcommand};
use greenfloor_engine::cli_util::emit_engine_cli_error;
use greenfloor_engine::coinset::parse_coin_ids;
use greenfloor_engine::coinset_cli::{run_coinset_command, CoinsetCliArgs};
use greenfloor_engine::config::load_signer_config;
use greenfloor_engine::daemon::{
    run_daemon_command, run_daemon_once_from_request_json, DaemonCliArgs, DaemonOnceJsonArgs,
};
use greenfloor_engine::error::SignerError;
use greenfloor_engine::hex::hex_to_bytes32;
use greenfloor_engine::hex_cli::{run_hex_command, HexCliArgs};
use greenfloor_engine::kms_cli::{run_kms_public_key_compressed_hex, KmsPublicKeyArgs};
use greenfloor_engine::offer::{build_vault_cat_offer, CreateOfferRequest};
use greenfloor_engine::vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest,
};
use greenfloor_engine::vault_coinset_scan::{
    run_vault_coinset_scan_command, VaultCoinsetScanCliArgs,
};
use greenfloor_engine::{resolve_vault_context, Error};

#[derive(Debug, Parser)]
#[command(
    name = "greenfloor-engine",
    about = "GreenFloor Rust engine: vault KMS signing and low-level ops"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Fetch vault custody metadata, derive vault puzzle hashes, and validate KMS key.
    VaultInfo {
        #[arg(long, default_value = "config/program.yaml")]
        config: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Vault CAT mixed split (aliases: split-cat, send-cat, combine-cat).
    #[command(
        name = "mixed-cat",
        visible_aliases = ["split-cat", "send-cat", "combine-cat"]
    )]
    MixedCat {
        #[arg(long, default_value = "config/program.yaml")]
        config: PathBuf,
        #[arg(long)]
        receive_address: String,
        #[arg(long)]
        asset_id: String,
        #[arg(long, value_delimiter = ',')]
        output_amounts: Vec<u64>,
        #[arg(long, value_delimiter = ',')]
        coin_ids: Vec<String>,
        #[arg(long)]
        allow_sub_cat_output: bool,
        #[arg(long)]
        broadcast: bool,
        #[arg(long)]
        json: bool,
    },
    /// Create a vault-signed CAT offer. Use --split-input-coins when input exceeds offer amount.
    CreateOffer {
        #[arg(long, default_value = "config/program.yaml")]
        config: PathBuf,
        #[arg(long)]
        receive_address: String,
        #[arg(long)]
        offer_asset_id: String,
        #[arg(long)]
        offer_amount: u64,
        #[arg(long)]
        request_asset_id: String,
        #[arg(long)]
        request_amount: u64,
        #[arg(long, value_delimiter = ',')]
        offer_coin_ids: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        presplit_coin_ids: Vec<String>,
        /// When selected CAT inputs exceed `--offer-amount`, split vault inputs before
        /// building the offer. If selected inputs already equal `--offer-amount` exactly,
        /// execution uses the direct offer path (no presplit spend) and `execution_mode`
        /// is `direct` even when this flag is set.
        #[arg(long)]
        split_input_coins: bool,
        #[arg(long)]
        broadcast_split: bool,
        #[arg(long)]
        expires_at: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    /// Run the `GreenFloor` daemon loop or a single cycle.
    Daemon(DaemonCliArgs),
    /// Run one daemon cycle from a JSON request file (integration tests and tooling).
    DaemonOnce(DaemonOnceJsonArgs),
    /// Coinset script IO: generic post RPC and push-tx for spend-bundle hex.
    Coinset(CoinsetCliArgs),
    /// Shared hex helpers for vault scan scripts.
    Hex(HexCliArgs),
    /// KMS helpers for one-off vault onboarding scripts.
    KmsPublicKeyCompressedHex(KmsPublicKeyArgs),
    /// Vault Coinset coin scan (nonce member puzzle hash discovery).
    VaultCoinsetScan(VaultCoinsetScanCliArgs),
}

#[tokio::main]
async fn main() {
    let json_mode = std::env::args().any(|arg| arg == "--json");
    if let Err(err) = Box::pin(run()).await {
        emit_engine_cli_error(&err, json_mode);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Error> {
    match Cli::parse().command {
        Commands::VaultInfo { config, json } => run_vault_info_command(config, json).await,
        Commands::MixedCat {
            config,
            receive_address,
            asset_id,
            output_amounts,
            coin_ids,
            allow_sub_cat_output,
            broadcast,
            json,
        } => {
            run_mixed_cat_command(
                config,
                receive_address,
                asset_id,
                output_amounts,
                coin_ids,
                allow_sub_cat_output,
                broadcast,
                json,
            )
            .await
        }
        Commands::CreateOffer {
            config,
            receive_address,
            offer_asset_id,
            offer_amount,
            request_asset_id,
            request_amount,
            offer_coin_ids,
            presplit_coin_ids,
            split_input_coins,
            broadcast_split,
            expires_at,
            json,
        } => {
            Box::pin(run_create_offer_command(
                config,
                receive_address,
                offer_asset_id,
                offer_amount,
                request_asset_id,
                request_amount,
                offer_coin_ids,
                presplit_coin_ids,
                split_input_coins,
                broadcast_split,
                expires_at,
                json,
            ))
            .await
        }
        Commands::Daemon(args) => Box::pin(run_daemon_cli_command(args)).await,
        Commands::DaemonOnce(args) => Box::pin(run_daemon_once_cli_command(args)).await,
        Commands::Coinset(args) => run_coinset_command(args).await,
        Commands::Hex(args) => run_hex_command(args),
        Commands::KmsPublicKeyCompressedHex(args) => run_kms_public_key_compressed_hex(args).await,
        Commands::VaultCoinsetScan(args) => run_vault_coinset_scan_command(args).await,
    }
}

async fn run_vault_info_command(config: PathBuf, json: bool) -> Result<(), Error> {
    let config = load_signer_config(&config)?;
    let context = resolve_vault_context(config).await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&context)
                .map_err(|err| SignerError::Other(format!("json encode failed: {err}")))?
        );
    } else {
        print_vault_info(&context);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_mixed_cat_command(
    config: PathBuf,
    receive_address: String,
    asset_id: String,
    output_amounts: Vec<u64>,
    coin_ids: Vec<String>,
    allow_sub_cat_output: bool,
    broadcast: bool,
    json: bool,
) -> Result<(), Error> {
    let result = run_mixed_split(
        &config,
        receive_address,
        asset_id,
        output_amounts,
        coin_ids,
        allow_sub_cat_output,
        broadcast,
    )
    .await?;
    print_mixed_split_result(&result, json)
}

async fn run_daemon_cli_command(args: DaemonCliArgs) -> Result<(), Error> {
    let code = Box::pin(run_daemon_command(args)).await?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

async fn run_daemon_once_cli_command(args: DaemonOnceJsonArgs) -> Result<(), Error> {
    let code = run_daemon_once_from_request_json(args).await?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_create_offer_command(
    config_path: PathBuf,
    receive_address: String,
    offer_asset_id: String,
    offer_amount: u64,
    request_asset_id: String,
    request_amount: u64,
    offer_coin_ids: Vec<String>,
    presplit_coin_ids: Vec<String>,
    split_input_coins: bool,
    broadcast_split: bool,
    expires_at: Option<u64>,
    json: bool,
) -> Result<(), Error> {
    let config = load_signer_config(&config_path)?;
    let parsed_offer_coin_ids = if offer_coin_ids.is_empty() {
        Vec::new()
    } else {
        parse_coin_ids(&offer_coin_ids)?
    };
    let parsed_presplit_coin_ids = if presplit_coin_ids.is_empty() {
        Vec::new()
    } else {
        parse_coin_ids(&presplit_coin_ids)?
    };
    let result = build_vault_cat_offer(
        config,
        CreateOfferRequest {
            receive_address,
            offer_asset_id,
            offer_amount,
            request_asset_id,
            request_amount,
            offer_coin_ids: parsed_offer_coin_ids,
            presplit_coin_ids: parsed_presplit_coin_ids,
            split_input_coins,
            broadcast_split,
            expires_at,
        },
    )
    .await?;
    print_create_offer_result(&result, json)
}

async fn run_mixed_split(
    config_path: &std::path::Path,
    receive_address: String,
    asset_id: String,
    output_amounts: Vec<u64>,
    coin_ids: Vec<String>,
    allow_sub_cat_output: bool,
    broadcast: bool,
) -> Result<greenfloor_engine::vault::MixedSplitResult, Error> {
    let config = load_signer_config(config_path)?;
    let parsed_coin_ids = if coin_ids.is_empty() {
        Vec::new()
    } else {
        parse_coin_ids(&coin_ids)?
    };
    build_and_optionally_broadcast_vault_cat_mixed_split(
        config,
        MixedSplitRequest {
            receive_address,
            asset_id: hex_to_bytes32(&asset_id)?,
            output_amounts,
            coin_ids: parsed_coin_ids,
            allow_sub_cat_output,
            fee_mojos: 0,
        },
        broadcast,
    )
    .await
}

fn print_vault_info(context: &greenfloor_engine::vault::VaultContext) {
    println!("network: {}", context.network);
    println!("launcher_id: {}", context.launcher_id);
    println!("inner_puzzle_hash: {}", context.inner_puzzle_hash);
    println!(
        "p2_singleton_message_hash (nonce 0): {}",
        context.p2_singleton_message_hash
    );
    println!("custody_hash: {}", context.custody_hash);
    println!("recovery_hash: {}", context.recovery_hash);
    println!("custody_threshold: {}", context.custody_threshold);
    println!("recovery_threshold: {}", context.recovery_threshold);
    println!(
        "recovery_clawback_timelock: {}",
        context.recovery_clawback_timelock
    );
    println!("kms_public_key_hex: {}", context.kms_public_key_hex);
    println!("kms_custody_key_match: {}", context.kms_custody_key_match);
    println!("secp256r1_custody_keys:");
    for key in &context.secp256r1_custody_keys {
        println!("  - {key}");
    }
}

fn print_mixed_split_result(
    result: &greenfloor_engine::vault::MixedSplitResult,
    json: bool,
) -> Result<(), Error> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(result)
                .map_err(|err| { SignerError::Other(format!("json encode failed: {err}")) })?
        );
        return Ok(());
    }
    println!("offered_total: {}", result.offered_total);
    println!("target_total: {}", result.target_total);
    println!("change_amount: {}", result.change_amount);
    println!("selected_coin_ids:");
    for coin_id in &result.selected_coin_ids {
        println!("  - {coin_id}");
    }
    if let Some(status) = &result.broadcast_status {
        println!("broadcast_status: {status}");
    }
    println!("spend_bundle_hex: {}", result.spend_bundle_hex);
    Ok(())
}

fn print_create_offer_result(
    result: &greenfloor_engine::offer::CreateOfferResult,
    json: bool,
) -> Result<(), Error> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(result)
                .map_err(|err| { SignerError::Other(format!("json encode failed: {err}")) })?
        );
        return Ok(());
    }
    println!("execution_mode: {}", result.execution_mode);
    if let Some(split_hex) = &result.split_spend_bundle_hex {
        println!("split_spend_bundle_hex: {split_hex}");
    }
    if let Some(presplit_coin_id) = &result.presplit_coin_id {
        println!("presplit_coin_id: {presplit_coin_id}");
    }
    if let Some(status) = &result.split_broadcast_status {
        println!("split_broadcast_status: {status}");
    }
    println!("offer_nonce: {}", result.offer_nonce);
    println!("selected_coin_ids:");
    for coin_id in &result.selected_coin_ids {
        println!("  - {coin_id}");
    }
    println!("offer: {}", result.offer);
    println!("spend_bundle_hex: {}", result.spend_bundle_hex);
    Ok(())
}
