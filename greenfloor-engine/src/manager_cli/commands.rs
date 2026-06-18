//! Clap definitions for the native manager CLI (`greenfloor-manager` binary).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "greenfloor-manager", about = "GreenFloor native manager CLI")]
pub struct ManagerCli {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, default_value = "config/markets.yaml")]
    pub markets_config: PathBuf,
    #[arg(long, default_value = "")]
    pub testnet_markets_config: String,
    #[arg(long, default_value = "config/cats.yaml")]
    pub cats_config: PathBuf,
    #[arg(long, default_value = "")]
    pub state_db: String,
    #[arg(long, help = "Emit compact single-line JSON output.")]
    pub json: bool,
    #[arg(
        long,
        help = "Override Dexie API base URL for cats and offer commands."
    )]
    pub dexie_base_url: Option<String>,
    #[command(subcommand)]
    pub command: ManagerCommands,
}

#[derive(Debug, Subcommand)]
pub enum ManagerCommands {
    ConfigValidate {
        #[arg(long, help = "Validate program.yaml only; skip markets overlay.")]
        program_only: bool,
    },
    KeysOnboard {
        #[arg(long, default_value = "")]
        chia_keys_dir: String,
        #[arg(long)]
        key_id: String,
        #[arg(long, default_value = "~/.greenfloor/state")]
        state_dir: PathBuf,
    },
    BuildAndPostOffer {
        #[arg(long)]
        market_id: Option<String>,
        #[arg(long)]
        pair: Option<String>,
        #[arg(long)]
        size_base_units: u64,
        #[arg(long, default_value_t = 1)]
        repeat: u32,
        #[arg(long, default_value = "mainnet")]
        network: String,
        #[arg(long)]
        dexie_base_url: Option<String>,
        #[arg(long)]
        allow_take: bool,
        #[arg(long)]
        claim_rewards: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        venue: Option<String>,
        #[arg(long)]
        splash_base_url: Option<String>,
    },
    Doctor,
    OffersStatus {
        #[arg(long, default_value = "")]
        market_id: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 30)]
        events_limit: usize,
    },
    OffersReconcile {
        #[arg(long, default_value = "")]
        market_id: String,
        #[arg(long, default_value_t = 200)]
        limit: usize,
        #[arg(long)]
        venue: Option<String>,
    },
    OffersCancel {
        #[arg(long, action = clap::ArgAction::Append)]
        offer_id: Vec<String>,
        #[arg(long)]
        cancel_open: bool,
        #[arg(long)]
        venue: Option<String>,
    },
    BootstrapHome {
        #[arg(long, default_value = "~/.greenfloor")]
        home_dir: PathBuf,
        #[arg(long, default_value = "config/program.yaml")]
        program_template: PathBuf,
        #[arg(long, default_value = "config/markets.yaml")]
        markets_template: PathBuf,
        #[arg(long, default_value = "config/cats.yaml")]
        cats_template: String,
        #[arg(long, default_value = "config/testnet-markets.yaml")]
        testnet_markets_template: String,
        #[arg(long)]
        seed_testnet_markets: bool,
        #[arg(long)]
        force: bool,
    },
    CatsAdd {
        #[arg(long, default_value = "mainnet")]
        network: String,
        #[arg(long)]
        cat_id: Option<String>,
        #[arg(long)]
        ticker: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        base_symbol: Option<String>,
        #[arg(long)]
        ticker_id: Option<String>,
        #[arg(long)]
        pool_id: Option<String>,
        #[arg(long)]
        last_price_xch: Option<String>,
        #[arg(long)]
        target_usd_per_unit: Option<String>,
        #[arg(long)]
        no_dexie_lookup: bool,
        #[arg(long)]
        replace: bool,
    },
    CatsList,
    CatsDelete {
        #[arg(long, default_value = "mainnet")]
        network: String,
        #[arg(long)]
        cat_id: Option<String>,
        #[arg(long)]
        ticker: Option<String>,
        #[arg(long)]
        no_dexie_lookup: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        preflight_only: bool,
    },
    SetLogLevel {
        #[arg(long)]
        log_level: String,
    },
    CoinsList {
        #[arg(long, default_value = "")]
        asset: String,
        #[arg(long, default_value = "")]
        vault_id: String,
        #[arg(long, default_value = "")]
        cat_id: String,
    },
    CoinStatus {
        #[arg(long, default_value = "")]
        asset: String,
        #[arg(long, default_value = "")]
        vault_id: String,
        #[arg(long, default_value = "")]
        cat_id: String,
    },
    CoinSplit {
        #[arg(long)]
        market_id: Option<String>,
        #[arg(long)]
        pair: Option<String>,
        #[arg(long, default_value = "mainnet")]
        network: String,
        #[arg(long, action = clap::ArgAction::Append)]
        coin_id: Vec<String>,
        #[arg(long, default_value_t = 0)]
        amount_per_coin: i64,
        #[arg(long, default_value_t = 0)]
        number_of_coins: i64,
        #[arg(long, default_value_t = 0)]
        size_base_units: i64,
        #[arg(long)]
        until_ready: bool,
        #[arg(long, default_value_t = 3)]
        max_iterations: i32,
        #[arg(long)]
        no_wait: bool,
        #[arg(long)]
        allow_lock_all_spendable: bool,
        #[arg(long)]
        force_split_when_ready: bool,
    },
    CoinCombine {
        #[arg(long)]
        market_id: Option<String>,
        #[arg(long)]
        pair: Option<String>,
        #[arg(long, default_value = "mainnet")]
        network: String,
        #[arg(long, default_value_t = 0)]
        input_coin_count: i64,
        #[arg(long, default_value = "")]
        asset_id: String,
        #[arg(long, action = clap::ArgAction::Append)]
        coin_id: Vec<String>,
        #[arg(long, default_value_t = 0)]
        size_base_units: i64,
        #[arg(long)]
        until_ready: bool,
        #[arg(long, default_value_t = 3)]
        max_iterations: i32,
        #[arg(long)]
        no_wait: bool,
    },
}
