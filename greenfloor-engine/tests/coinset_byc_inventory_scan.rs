//! Live Coinset BYC inventory scan regression (manual / CI optional).

use chia_protocol::Bytes32;
use greenfloor_engine::coinset::direct_coinset_client;
use greenfloor_engine::coinset::list_unspent_cats;

const BYC_ASSET_HEX: &str = "ae1536f56760e471ad85ead45f00d680ff9cca73b8cc3407be778f1c0c606eac";
const RECEIVE_ADDRESS: &str = "xch1u3tytpv45sj0h4lpwmtkyzh2ggvw4x7jccyxzu995p2aj40wzcxqvymyn3";

#[tokio::test]
#[ignore = "live coinset BYC inventory scan (run: RUST_BACKTRACE=1 cargo test -p greenfloor-engine byc_inventory_scan_live_coinset -- --ignored --nocapture)"]
async fn byc_inventory_scan_live_coinset() {
    let asset_bytes = hex::decode(BYC_ASSET_HEX).expect("BYC asset hex");
    let asset_id = Bytes32::try_from(asset_bytes.as_slice()).expect("BYC asset id");
    let client = direct_coinset_client("mainnet", None).expect("coinset client");
    let cats = list_unspent_cats(&client, RECEIVE_ADDRESS, asset_id)
        .await
        .expect("BYC inventory scan must return SignerResult, not panic");
    eprintln!("BYC inventory scan: {} spendable cats", cats.len());
}
