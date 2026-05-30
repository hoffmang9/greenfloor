use std::path::PathBuf;

use clap::{Parser, Subcommand};
use greenfloor_engine::daemon::{
    default_testnet_markets_path, initialize_daemon_file_logging, load_daemon_program_runtime,
    resolve_testnet_markets_path, run_daemon_cycle_once, warn_if_daemon_log_level_auto_healed,
    DaemonDispatchState, DaemonInstanceLock, DaemonRunOnceRequest,
};
use greenfloor_engine::vault::members::hex_to_bytes32;
use greenfloor_engine::{
    build_and_optionally_broadcast_vault_cat_mixed_split, build_and_post_offer,
    build_vault_cat_offer, load_signer_config, parse_coin_ids, resolve_vault_context,
    BuildAndPostOfferRequest, CreateOfferRequest, MixedSplitRequest,
};

#[derive(Debug, Parser)]
#[command(
    name = "greenfloor-engine",
    about = "GreenFloor Rust engine: vault KMS signing and manager CLI"
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
    /// Build a vault-signed offer and post it to Dexie or Splash (manager CLI path).
    BuildAndPostOffer {
        #[arg(long, default_value = "config/program.yaml")]
        program_config: PathBuf,
        #[arg(long, default_value = "config/markets.yaml")]
        markets_config: PathBuf,
        #[arg(long, default_value = "")]
        testnet_markets_config: PathBuf,
        #[arg(long, default_value = "mainnet")]
        network: String,
        #[arg(long)]
        market_id: Option<String>,
        #[arg(long)]
        pair: Option<String>,
        #[arg(long)]
        size_base_units: u64,
        #[arg(long, default_value = "1")]
        repeat: u32,
        #[arg(long)]
        venue: Option<String>,
        #[arg(long)]
        dexie_base_url: Option<String>,
        #[arg(long)]
        splash_base_url: Option<String>,
        #[arg(long)]
        allow_take: bool,
        #[arg(long)]
        claim_rewards: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        no_persist_results: bool,
    },
    /// Daemon cycle orchestration (native Rust entry).
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
}

#[derive(Debug, Subcommand)]
enum DaemonCommands {
    /// Run one daemon evaluation cycle and exit.
    RunOnce {
        #[arg(long, default_value = "config/program.yaml")]
        program_config: PathBuf,
        #[arg(long, default_value = "config/markets.yaml")]
        markets_config: PathBuf,
        #[arg(long)]
        testnet_markets_config: Option<PathBuf>,
        #[arg(long, default_value = "")]
        key_ids: String,
        #[arg(long, default_value = "")]
        state_db: String,
        #[arg(long, default_value = "https://api.coinset.org")]
        coinset_base_url: String,
        #[arg(long, default_value = "~/.greenfloor/state")]
        state_dir: PathBuf,
        /// Emit full cycle response JSON on stdout (exit code still reflects cycle outcome).
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "0")]
        dispatch_cursor: usize,
        #[arg(long, default_value = "")]
        dispatch_requeue_ids: String,
        #[arg(long, default_value_t = false)]
        poll_coinset_mempool: bool,
        #[arg(long, default_value_t = false)]
        use_websocket_capture: bool,
    },
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), greenfloor_engine::Error> {
    let cli = Cli::parse();
    match cli.command {
        Commands::VaultInfo { config, json } => {
            let config = load_signer_config(&config)?;
            let context = resolve_vault_context(config).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&context).map_err(|err| {
                        greenfloor_engine::Error::Other(format!("json encode failed: {err}"))
                    })?
                );
            } else {
                print_vault_info(&context);
            }
        }
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
            print_mixed_split_result(&result, json)?;
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
            let config = load_signer_config(&config)?;
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
            print_create_offer_result(&result, json)?;
        }
        Commands::BuildAndPostOffer {
            program_config,
            markets_config,
            testnet_markets_config,
            network,
            market_id,
            pair,
            size_base_units,
            repeat,
            venue,
            dexie_base_url,
            splash_base_url,
            allow_take,
            claim_rewards,
            dry_run,
            json,
            no_persist_results,
        } => {
            if market_id.is_none() == pair.is_none() {
                return Err(greenfloor_engine::Error::Other(
                    "provide exactly one of --market-id or --pair".to_string(),
                ));
            }
            let testnet_overlay = if testnet_markets_config.as_os_str().is_empty() {
                None
            } else {
                Some(testnet_markets_config)
            };
            let response = build_and_post_offer(BuildAndPostOfferRequest {
                program_path: program_config,
                markets_path: markets_config,
                testnet_markets_path: testnet_overlay,
                network,
                market_id,
                pair,
                size_base_units,
                repeat,
                publish_venue: venue,
                dexie_base_url,
                splash_base_url,
                drop_only: !allow_take,
                claim_rewards,
                dry_run,
                compact_json: json,
                persist_results: !no_persist_results,
                action_side: None,
            })
            .await?;
            println!("{}", response.output);
            if response.exit_code != 0 {
                std::process::exit(response.exit_code);
            }
        }
        Commands::Daemon { command } => match command {
            DaemonCommands::RunOnce {
                program_config,
                markets_config,
                testnet_markets_config,
                key_ids,
                state_db,
                coinset_base_url,
                state_dir,
                json,
                dispatch_cursor,
                dispatch_requeue_ids,
                poll_coinset_mempool,
                use_websocket_capture,
            } => {
                let exit_code = run_daemon_cli_once(
                    program_config,
                    markets_config,
                    testnet_markets_config,
                    key_ids,
                    state_db,
                    coinset_base_url,
                    state_dir,
                    json,
                    dispatch_cursor,
                    dispatch_requeue_ids,
                    poll_coinset_mempool,
                    use_websocket_capture,
                )
                .await?;
                if exit_code != 0 {
                    std::process::exit(exit_code);
                }
            }
        },
    }
    Ok(())
}

async fn run_daemon_cli_once(
    program_config: PathBuf,
    markets_config: PathBuf,
    testnet_markets_config: Option<PathBuf>,
    key_ids: String,
    state_db: String,
    coinset_base_url: String,
    state_dir: PathBuf,
    json: bool,
    dispatch_cursor: usize,
    dispatch_requeue_ids: String,
    poll_coinset_mempool: bool,
    use_websocket_capture: bool,
) -> Result<i32, greenfloor_engine::Error> {
    let runtime = load_daemon_program_runtime(&program_config)?;
    initialize_daemon_file_logging(&runtime.home_dir, &runtime.app_log_level)?;
    warn_if_daemon_log_level_auto_healed(runtime.app_log_level_was_missing, &program_config);

    let expanded_state_dir = expand_user_path(&state_dir);
    let _lock = match DaemonInstanceLock::acquire(&expanded_state_dir, "once") {
        Ok(lock) => lock,
        Err(err) => {
            tracing::error!(event = "daemon_lock_conflict", error = %err);
            return Ok(3);
        }
    };

    tracing::info!(
        mode = "once",
        program_config = %program_config.display(),
        markets_config = %markets_config.display(),
        "daemon_starting"
    );

    let testnet_overlay = match testnet_markets_config {
        None => default_testnet_markets_path(),
        Some(path) if path.as_os_str().is_empty() => default_testnet_markets_path(),
        Some(path) => resolve_testnet_markets_path(&path.to_string_lossy()),
    };
    let allowed_key_ids: Vec<String> = key_ids
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    let state_db_override = if state_db.trim().is_empty() {
        None
    } else {
        Some(state_db.trim().to_string())
    };

    let immediate_requeue_ids: Vec<String> = dispatch_requeue_ids
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();

    let response = run_daemon_cycle_once(&DaemonRunOnceRequest {
        program_path: program_config,
        markets_path: markets_config,
        testnet_markets_path: testnet_overlay,
        state_db_override,
        coinset_base_url,
        state_dir: expanded_state_dir,
        poll_coinset_mempool,
        use_websocket_capture,
        allowed_key_ids,
        dispatch_state: DaemonDispatchState {
            cursor: dispatch_cursor,
            immediate_requeue_ids,
        },
    })
    .await?;

    if json {
        println!(
            "{}",
            serde_json::to_string(&response).map_err(|err| {
                greenfloor_engine::Error::Other(format!("json encode failed: {err}"))
            })?
        );
    }

    tracing::info!(
        mode = "once",
        exit_code = response.exit_code,
        "daemon_stopped"
    );
    Ok(response.exit_code)
}

fn expand_user_path(path: &PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path.clone()
}

async fn run_mixed_split(
    config_path: &std::path::Path,
    receive_address: String,
    asset_id: String,
    output_amounts: Vec<u64>,
    coin_ids: Vec<String>,
    allow_sub_cat_output: bool,
    broadcast: bool,
) -> Result<greenfloor_engine::MixedSplitResult, greenfloor_engine::Error> {
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
    result: &greenfloor_engine::MixedSplitResult,
    json: bool,
) -> Result<(), greenfloor_engine::Error> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).map_err(|err| {
                greenfloor_engine::Error::Other(format!("json encode failed: {err}"))
            })?
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
    result: &greenfloor_engine::CreateOfferResult,
    json: bool,
) -> Result<(), greenfloor_engine::Error> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).map_err(|err| {
                greenfloor_engine::Error::Other(format!("json encode failed: {err}"))
            })?
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
