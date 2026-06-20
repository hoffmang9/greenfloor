//! Manager CLI entrypoints for offers status, reconcile, and cancel.

use clap::Args;

use crate::cli_util::optional_trimmed;
use crate::config::load_program_config;
use crate::error::SignerResult;
use crate::offer::lifecycle::{
    offers_cancel_cli, offers_status_cli, reconcile_offers_cli, OffersCancelCliResult,
};
use crate::storage::resolve_state_db_path;

use super::context::ManagerContext;

#[derive(Debug, Args)]
pub struct OffersReconcileCliArgs {
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    #[arg(long)]
    pub venue: Option<String>,
}

#[derive(Debug, Args)]
pub struct OffersStatusCliArgs {
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    #[arg(long, default_value_t = 30)]
    pub events_limit: usize,
}

#[derive(Debug, Args)]
pub struct OffersCancelCliArgs {
    #[arg(long, action = clap::ArgAction::Append)]
    pub offer_id: Vec<String>,
    #[arg(long)]
    pub cancel_open: bool,
    #[arg(long)]
    pub venue: Option<String>,
}

/// Run offers reconcile command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_offers_reconcile_command(
    ctx: &ManagerContext,
    args: OffersReconcileCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
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
    ctx.emit_serialized(&payload)?;
    Ok(0)
}

/// Run offers status command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn run_offers_status_command(
    ctx: &ManagerContext,
    args: &OffersStatusCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
    let payload = offers_status_cli(
        &db_path,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
        args.events_limit,
    )?;
    ctx.emit_serialized(&payload)?;
    Ok(0)
}

/// Run offers cancel command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_offers_cancel_command(
    ctx: &ManagerContext,
    args: OffersCancelCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
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
    ctx.emit_serialized(&payload)?;
    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use clap::Parser;
    use serde_json::json;

    use crate::manager_cli::commands::ManagerCli;
    use crate::manager_cli::context::ManagerContext;
    use crate::manager_cli::json::ManagerOutput;
    use crate::minimal_program_template::{write_minimal_program, MinimalProgramParams};
    use crate::storage::SqliteStore;

    use super::*;

    fn write_offers_program(path: &Path, home_dir: &Path, dexie_api_base: &str) {
        write_minimal_program(
            path,
            MinimalProgramParams {
                home_dir,
                dexie_api_base,
                low_inventory_alerts_enabled: true,
                pushover_enabled: true,
                ..Default::default()
            },
        );
    }

    fn offers_context(
        program: PathBuf,
        markets: PathBuf,
        state_db: &str,
    ) -> (
        ManagerContext,
        std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) {
        let (output, captured) = ManagerOutput::capturing(true);
        let mut ctx = ManagerContext::for_test_with_output(program, markets, output);
        ctx.state_db = state_db.to_string();
        (ctx, captured)
    }

    fn pop_payload(
        captured: &std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) -> serde_json::Value {
        captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted")
    }

    fn seed_offer_states(db_path: &Path, rows: &[(&str, &str, &str)]) {
        let store = SqliteStore::open(db_path).expect("open db");
        for (offer_id, market_id, state) in rows {
            store
                .upsert_offer_state(offer_id, market_id, state, Some(0))
                .expect("seed offer");
        }
    }

    #[tokio::test]
    async fn offers_reconcile_updates_states_from_dexie() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "markets: []\n").expect("write markets");
        let db_path = dir.path().join("state.sqlite");
        let confirmed_tx_id = "a".repeat(64);
        {
            let store = SqliteStore::open(&db_path).expect("open db");
            store
                .upsert_offer_state("offer-ok", "m1", "open", Some(0))
                .expect("seed");
            store
                .upsert_offer_state("offer-missing", "m1", "open", Some(0))
                .expect("seed");
            store
                .observe_mempool_tx_ids(std::slice::from_ref(&confirmed_tx_id))
                .expect("observe");
            store.confirm_tx_ids(&[confirmed_tx_id]).expect("confirm");
        }

        let mut server = mockito::Server::new_async().await;
        let confirmed_tx_id = "a".repeat(64);
        let _ok = server
            .mock("GET", "/v1/offers/offer-ok")
            .with_status(200)
            .with_body(json!({"id":"offer-ok","status":4,"tx_id": confirmed_tx_id}).to_string())
            .create_async()
            .await;
        let _missing = server
            .mock("GET", "/v1/offers/offer-missing")
            .with_status(404)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create_async()
            .await;
        write_offers_program(&program, dir.path(), &server.url());
        let (ctx, captured) = offers_context(program, markets, db_path.to_str().expect("db path"));
        let code = run_offers_reconcile_command(
            &ctx,
            OffersReconcileCliArgs {
                market_id: String::new(),
                limit: 20,
                venue: Some("dexie".to_string()),
            },
        )
        .await
        .expect("offers-reconcile");
        assert_eq!(code, 0);
        let payload = pop_payload(&captured);
        assert_eq!(payload.get("reconciled_count"), Some(&json!(2)));
        assert_eq!(payload.get("changed_count"), Some(&json!(2)));

        let store = SqliteStore::open(&db_path).expect("open db");
        let rows = store.list_offer_states(None, 20).expect("rows");
        let by_id: HashMap<_, _> = rows
            .iter()
            .map(|row| (row.offer_id.as_str(), row))
            .collect();
        assert_eq!(
            by_id.get("offer-ok").expect("offer-ok").state,
            "tx_block_confirmed"
        );
        assert_eq!(
            by_id.get("offer-missing").expect("missing").state,
            "expired"
        );
    }

    #[tokio::test]
    async fn offers_cancel_cancel_open_uses_dexie() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "markets: []\n").expect("write markets");
        let db_path = dir.path().join("db").join("greenfloor.sqlite");
        write_offers_program(&program, dir.path(), "https://api.dexie.space");
        seed_offer_states(
            &db_path,
            &[
                ("offer-open", "m1", "open"),
                ("offer-expired", "m1", "expired"),
            ],
        );

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/v1/offers/offer-open/cancel")
            .with_status(200)
            .with_body(r#"{"success":true,"id":"offer-open","status":3}"#)
            .create_async()
            .await;
        write_offers_program(&program, dir.path(), &server.url());
        let (ctx, captured) = offers_context(program, markets, "");
        let code = run_offers_cancel_command(
            &ctx,
            OffersCancelCliArgs {
                offer_id: vec![],
                cancel_open: true,
                venue: None,
            },
        )
        .await
        .expect("offers-cancel");
        assert_eq!(code, 0);
        let payload = pop_payload(&captured);
        assert_eq!(payload.get("venue"), Some(&json!("dexie")));
        assert_eq!(payload.get("selected_count"), Some(&json!(1)));
        assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
        assert_eq!(payload.get("failed_count"), Some(&json!(0)));

        let store = SqliteStore::open(&db_path).expect("open db");
        let rows = store.list_offer_states(None, 10).expect("rows");
        let by_id: HashMap<_, _> = rows
            .iter()
            .map(|row| (row.offer_id.as_str(), row))
            .collect();
        assert_eq!(by_id.get("offer-open").expect("open").state, "cancelled");
        assert_eq!(
            by_id.get("offer-open").expect("open").last_seen_status,
            Some(3)
        );
        assert_eq!(
            by_id.get("offer-expired").expect("expired").state,
            "expired"
        );
    }

    #[tokio::test]
    async fn offers_cancel_by_offer_id_uses_dexie() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "markets: []\n").expect("write markets");
        let db_path = dir.path().join("db").join("greenfloor.sqlite");
        write_offers_program(&program, dir.path(), "https://api.dexie.space");
        seed_offer_states(
            &db_path,
            &[
                ("offer-target", "m1", "open"),
                ("offer-other", "m1", "open"),
            ],
        );

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/v1/offers/offer-target/cancel")
            .with_status(200)
            .with_body(r#"{"success":true,"id":"offer-target","status":3}"#)
            .create_async()
            .await;
        write_offers_program(&program, dir.path(), &server.url());
        let (ctx, captured) = offers_context(program, markets, "");
        let code = run_offers_cancel_command(
            &ctx,
            OffersCancelCliArgs {
                offer_id: vec!["offer-target".to_string()],
                cancel_open: false,
                venue: None,
            },
        )
        .await
        .expect("offers-cancel");
        assert_eq!(code, 0);
        let payload = pop_payload(&captured);
        assert_eq!(payload.get("selected_count"), Some(&json!(1)));
        assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
    }

    #[tokio::test]
    async fn offers_cancel_reports_dexie_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "markets: []\n").expect("write markets");
        let db_path = dir.path().join("db").join("greenfloor.sqlite");
        write_offers_program(&program, dir.path(), "https://api.dexie.space");
        seed_offer_states(&db_path, &[("offer-fail", "m1", "open")]);

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/v1/offers/offer-fail/cancel")
            .with_status(200)
            .with_body(r#"{"success":false,"error":"not_found"}"#)
            .create_async()
            .await;
        write_offers_program(&program, dir.path(), &server.url());
        let (ctx, captured) = offers_context(program, markets, "");
        let code = run_offers_cancel_command(
            &ctx,
            OffersCancelCliArgs {
                offer_id: vec!["offer-fail".to_string()],
                cancel_open: false,
                venue: None,
            },
        )
        .await
        .expect("offers-cancel");
        assert_eq!(code, 2);
        let payload = pop_payload(&captured);
        assert_eq!(payload.get("cancelled_count"), Some(&json!(0)));
        assert_eq!(payload.get("failed_count"), Some(&json!(1)));
    }

    #[test]
    fn offers_cancel_rejects_removed_submit_onchain_flag() {
        assert!(ManagerCli::try_parse_from([
            "greenfloor-manager",
            "--program-config",
            "/tmp/program.yaml",
            "offers-cancel",
            "--offer-id",
            "offer-1",
            "--submit-onchain-after-offchain",
        ])
        .is_err());
    }
}
