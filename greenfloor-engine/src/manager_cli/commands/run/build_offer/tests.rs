use crate::manager_cli::commands::clap::ManagerCommands;
use crate::manager_cli::test_support::{
    pop_json, write_program_with_signer, ManagerContextBuilder,
};
use crate::offer::operator::BuildOfferTestOverrides;

use super::{build_and_post_request, run_command, run_command_with_test_overrides};

#[test]
fn build_and_post_request_maps_cli_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_program_with_signer(&program, dir.path());
    std::fs::write(&markets, "markets: []\n").expect("write markets");
    let harness = ManagerContextBuilder::new(program.clone(), markets.clone())
        .scratch_dir(dir.path().to_path_buf())
        .dexie_base_url("https://dexie.example")
        .build_capturing();
    let command = ManagerCommands::BuildAndPostOffer {
        market_id: Some("m1".to_string()),
        pair: None,
        size_base_units: 42,
        repeat: 2,
        network: "mainnet".to_string(),
        dexie_base_url: None,
        allow_take: true,
        claim_rewards: true,
        dry_run: true,
        venue: Some("dexie".to_string()),
        splash_base_url: Some("https://splash.example".to_string()),
    };

    let request = build_and_post_request(&command, &harness.ctx).expect("build request");
    assert_eq!(request.program_path, program);
    assert_eq!(request.markets_path, markets);
    assert_eq!(request.network, "mainnet");
    assert_eq!(request.market_id, Some("m1".to_string()));
    assert_eq!(request.size_base_units, 42);
    assert_eq!(request.repeat, 2);
    assert_eq!(
        request.dexie_base_url.as_deref(),
        Some("https://dexie.example")
    );
    assert_eq!(
        request.splash_base_url.as_deref(),
        Some("https://splash.example")
    );
    assert!(!request.venue.drop_only);
    assert!(request.venue.claim_rewards);
    assert!(request.run.dry_run);
}

#[tokio::test]
async fn run_command_requires_market_selector() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = ManagerContextBuilder::new(
        dir.path().join("program.yaml"),
        dir.path().join("markets.yaml"),
    )
    .scratch_dir(dir.path().to_path_buf())
    .build_capturing();
    let err = run_command(
        ManagerCommands::BuildAndPostOffer {
            market_id: None,
            pair: None,
            size_base_units: 1,
            repeat: 1,
            network: "mainnet".to_string(),
            dexie_base_url: None,
            allow_take: false,
            claim_rewards: false,
            dry_run: true,
            venue: None,
            splash_base_url: None,
        },
        &harness.ctx,
    )
    .await
    .expect_err("missing market selector");
    assert!(err
        .to_string()
        .contains("provide exactly one of --market-id or --pair"));
}

#[tokio::test]
async fn run_command_dry_run_emits_preview_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_program_with_signer(&program, dir.path());
    std::fs::write(
        &markets,
        include_str!("../../../../../tests/fixtures/data/build_offer_markets.yaml"),
    )
    .expect("write markets fixture");
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let command = ManagerCommands::BuildAndPostOffer {
        market_id: Some("m1".to_string()),
        pair: None,
        size_base_units: 1,
        repeat: 1,
        network: "mainnet".to_string(),
        dexie_base_url: None,
        allow_take: false,
        claim_rewards: false,
        dry_run: true,
        venue: None,
        splash_base_url: None,
    };
    let code = run_command_with_test_overrides(
        command,
        &harness.ctx,
        BuildOfferTestOverrides {
            offer_text: Some("offer1dryrunpreviewstub".to_string()),
        },
    )
    .await
    .expect("build-and-post-offer");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("dry_run"), Some(&serde_json::json!(true)));
}
