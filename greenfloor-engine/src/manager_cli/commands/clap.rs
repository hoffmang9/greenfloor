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
    ProgramFields,
    MarketsFields,
    CatsFields,
    MaterializeMinimalProgram {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        home_dir: PathBuf,
        #[arg(long, default_value = "https://api.dexie.space")]
        dexie_api_base: String,
        #[arg(long, default_value = "INFO")]
        log_level: String,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(long, default_value_t = false)]
        low_inventory_alerts_enabled: bool,
        #[arg(long, default_value_t = false)]
        pushover_enabled: bool,
        #[arg(long, default_value_t = false)]
        with_signer: bool,
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
    AuditPrune {
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(
            long,
            default_value_t = false,
            help = "Run VACUUM after deleting rows."
        )]
        vacuum: bool,
    },
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
        #[arg(long, action = clap::ArgAction::Append, value_name = "PATH_OR_BECH32")]
        offer_file: Vec<String>,
        #[arg(long, value_name = "MARKET_ID")]
        market_id: Option<String>,
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
    /// List spendable coins for a market row.
    ///
    /// Without `--market-id` or `--pair`, selects the lexicographically smallest enabled
    /// market whose `receive_address` prefix matches `--network` (`xch1` / `txch1`).
    CoinsList {
        /// Market row id (exactly one of `--market-id` or `--pair` when set).
        #[arg(long)]
        market_id: Option<String>,
        /// Pair selector `base:quote` or `base/quote` (exactly one of `--market-id` or `--pair` when set).
        #[arg(long)]
        pair: Option<String>,
        #[arg(
            long,
            default_value = "mainnet",
            help = "Operator network for Coinset and receive-address filtering"
        )]
        network: String,
        #[arg(long, default_value = "")]
        asset: String,
        #[arg(long, default_value = "")]
        cat_id: String,
    },
    /// Summarize spendable coin counts for a market row (same market selection as [`CoinsList`]).
    CoinStatus {
        /// Market row id (exactly one of `--market-id` or `--pair` when set).
        #[arg(long)]
        market_id: Option<String>,
        /// Pair selector `base:quote` or `base/quote` (exactly one of `--market-id` or `--pair` when set).
        #[arg(long)]
        pair: Option<String>,
        #[arg(
            long,
            default_value = "mainnet",
            help = "Operator network for Coinset and receive-address filtering"
        )]
        network: String,
        #[arg(long, default_value = "")]
        asset: String,
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
    #[command(group(
        clap::ArgGroup::new("combine_execution")
            .args(["dry_run", "list_only"])
            .multiple(false)
    ))]
    CombineMarketCatDust {
        #[arg(long, default_value = "")]
        network: String,
        #[arg(long, default_value = "")]
        coinset_base_url: String,
        #[arg(long, default_value = "")]
        launcher_id: String,
        #[arg(long, default_value = "~/.greenfloor/cache/vault_launcher_id.txt")]
        launcher_id_file: String,
        #[arg(long, default_value_t = 1000)]
        dust_threshold_mojos: u64,
        #[arg(long, default_value_t = 10)]
        max_input_coins: usize,
        /// Cap combinable batches executed (or previewed) per job; omit to run the full plan.
        #[arg(long)]
        max_batches: Option<usize>,
        #[arg(long, default_value_t = 120)]
        max_nonce: u32,
        #[arg(long, default_value = "")]
        cat_asset_id: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        list_only: bool,
        #[arg(long, default_value_t = 15 * 60)]
        verify_timeout_seconds: u64,
        #[arg(long, default_value_t = 8)]
        verify_poll_seconds: u64,
    },
    /// Trace one asset from vault reception through intermediate coins to current balance.
    VaultAssetTrace {
        #[arg(
            long,
            help = "Asset to trace: xch/txch, CAT ticker, or CAT asset id hex"
        )]
        asset: String,
        #[arg(long, default_value = "mainnet")]
        network: String,
        #[arg(long, default_value = "")]
        coinset_base_url: String,
        #[arg(long, default_value = "")]
        launcher_id: String,
        #[arg(long, default_value = "~/.greenfloor/cache/vault_launcher_id.txt")]
        launcher_id_file: String,
        #[arg(long, default_value_t = 100)]
        max_nonce: u32,
    },
    #[command(hide = true, about = "Emit global/subcommand CLI flag groups as JSON")]
    FlagGroups {
        subcommand: String,
    },
}
