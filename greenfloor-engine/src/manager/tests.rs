use std::io::Write;

use mockito::Matcher;
use serde_json::json;

use crate::adapters::{post_offer_phase_dexie, DexieClient};
use crate::config::{
    load_markets_config, load_program_config, resolve_market_for_build, require_signer_offer_path,
};

#[tokio::test]
async fn dexie_post_offer_phase_posts_and_verifies_visibility() {
    let mut server = mockito::Server::new_async().await;
    let offer_id = "offer-123";
    let _post = server
        .mock("POST", "/v1/offers")
        .with_status(200)
        .with_body(json!({"success": true, "id": offer_id}).to_string())
        .create_async()
        .await;
    let _get = server
        .mock("GET", Matcher::Regex(r"/v1/offers/.*".to_string()))
        .with_status(200)
        .with_body(
            json!({
                "offer": {
                    "id": offer_id,
                    "offered": [{"id": "basecat"}],
                    "requested": [{"code": "xch"}],
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let dexie = DexieClient::new(server.url());
    let result = post_offer_phase_dexie(
        &dexie,
        "offer1test",
        true,
        false,
        "basecat",
        "A1",
        "xch",
        "xch",
    )
    .await
    .expect("post");
    assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(result.get("id").and_then(|v| v.as_str()), Some(offer_id));
}

#[test]
fn manager_config_and_market_resolution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program_path = dir.path().join("program.yaml");
    let markets_path = dir.path().join("markets.yaml");
    let mut program_file = std::fs::File::create(&program_path).expect("create");
    write!(
        program_file,
        r#"
app:
  network: mainnet
  home_dir: /tmp/gf
runtime:
  offer_bootstrap_wait_timeout_seconds: 120
venues:
  dexie:
    api_base: https://api.dexie.space
  splash:
    api_base: http://localhost:4000
  offer_publish:
    provider: dexie
coin_ops:
  minimum_fee_mojos: 10000000
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/demo
vault:
  launcher_id: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"
      curve: BLS12_381
"#
    )
    .expect("write");
    std::fs::write(
        &markets_path,
        r#"
markets:
  - id: m1
    enabled: true
    base_asset: a1
    base_symbol: A1
    quote_asset: xch
    receive_address: xch1test
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#,
    )
    .expect("write markets");

    require_signer_offer_path(&program_path).expect("signer path");
    let program = load_program_config(&program_path).expect("program");
    assert_eq!(program.offer_publish_venue, "dexie");
    let markets = load_markets_config(&markets_path).expect("markets");
    let market = resolve_market_for_build(&markets, Some("m1"), None, "mainnet").expect("market");
    assert_eq!(market.market_id, "m1");
}
