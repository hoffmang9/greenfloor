//! Manager CLI entrypoints for offers status, reconcile, and cancel.

use std::path::PathBuf;

use clap::Args;

use crate::config::load_program_config;
use crate::daemon::reconcile_offers_cli;
use crate::error::SignerResult;
use crate::storage::resolve_state_db_path;

use super::offer_lifecycle::{
    offers_cancel_cli, offers_status_cli, OffersCancelCliResult,
};

fn print_json(value: &impl serde::Serialize) -> SignerResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|err| {
            crate::error::SignerError::Other(format!("failed to encode json output: {err}"))
        })?
    );
    Ok(())
}

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Args)]
pub struct OffersReconcileCliArgs {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, default_value = "")]
    pub state_db: String,
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    #[arg(long)]
    pub venue: Option<String>,
}

#[derive(Debug, Args)]
pub struct OffersStatusCliArgs {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, default_value = "")]
    pub state_db: String,
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    #[arg(long, default_value_t = 30)]
    pub events_limit: usize,
}

#[derive(Debug, Args)]
pub struct OffersCancelCliArgs {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, action = clap::ArgAction::Append)]
    pub offer_id: Vec<String>,
    #[arg(long)]
    pub cancel_open: bool,
    #[arg(long)]
    pub venue: Option<String>,
}

pub async fn run_offers_reconcile_command(args: OffersReconcileCliArgs) -> SignerResult<i32> {
    let program = load_program_config(&args.program_config)?;
    let db_path = resolve_state_db_path(
        &program.home_dir,
        optional_trimmed(&args.state_db).as_deref(),
    );
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload = reconcile_offers_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
    )
    .await?;
    print_json(&payload)?;
    Ok(0)
}

pub fn run_offers_status_command(args: OffersStatusCliArgs) -> SignerResult<i32> {
    let program = load_program_config(&args.program_config)?;
    let db_path = resolve_state_db_path(
        &program.home_dir,
        optional_trimmed(&args.state_db).as_deref(),
    );
    let payload = offers_status_cli(
        &db_path,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
        args.events_limit,
    )?;
    print_json(&payload)?;
    Ok(0)
}

pub async fn run_offers_cancel_command(args: OffersCancelCliArgs) -> SignerResult<i32> {
    let program = load_program_config(&args.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, None);
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload: OffersCancelCliResult = offers_cancel_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        &args.offer_id,
        args.cancel_open,
    )
    .await?;
    let exit_code = if payload.failed_count == 0 { 0 } else { 2 };
    print_json(&payload)?;
    Ok(exit_code)
}
