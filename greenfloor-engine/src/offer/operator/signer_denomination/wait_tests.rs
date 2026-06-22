use std::collections::HashSet;

use serde_json::Value;

use super::{wait_for_coinset_confirmation, BootstrapWaitConfig};
use crate::coinset::coin_id_from_record;
use crate::test_support::signer_config::test_signer_config;

const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
const TEST_MIN_TIMEOUT_SECONDS: u64 = 1;

#[tokio::test]
async fn wait_for_coinset_confirmation_returns_new_coin_event() {
    let body = r#"{
        "success": true,
        "coin_records": [{
            "coin": {
                "parent_coin_info": "c325057d788bee13367cb8e2d71ff3e209b5e94b31b296322ba1a143053fef5b",
                "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                "amount": 1000
            },
            "coinbase": false,
            "confirmed_block_index": 1,
            "spent": false,
            "spent_block_index": 0,
            "timestamp": 1
        }]
    }"#;
    let record: Value = serde_json::from_str::<Value>(body)
        .expect("fixture")
        .get("coin_records")
        .and_then(|value| value.as_array())
        .and_then(|records| records.first())
        .cloned()
        .expect("record");
    let coin_id = coin_id_from_record(&record);
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(body)
        .create_async()
        .await;
    let signer = test_signer_config(&server.url());

    let events = wait_for_coinset_confirmation(BootstrapWaitConfig {
        network: "mainnet",
        signer: &signer,
        receive_address: RECEIVE_ADDRESS,
        asset_id: "xch",
        initial_coin_ids: &HashSet::new(),
        timeout_seconds: 5,
        min_timeout_seconds: TEST_MIN_TIMEOUT_SECONDS,
    })
    .await
    .expect("confirmed");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], "confirmed");
    assert_eq!(events[0]["coin_name"].as_str(), Some(coin_id.as_str()));
}

#[tokio::test]
async fn wait_for_coinset_confirmation_times_out_when_no_new_coins() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/get_coin_records_by_puzzle_hash")
        .with_status(200)
        .with_body(r#"{"success":true,"coin_records":[]}"#)
        .expect_at_least(1)
        .create_async()
        .await;
    let signer = test_signer_config(&server.url());

    let err = wait_for_coinset_confirmation(BootstrapWaitConfig {
        network: "mainnet",
        signer: &signer,
        receive_address: RECEIVE_ADDRESS,
        asset_id: "xch",
        initial_coin_ids: &HashSet::new(),
        timeout_seconds: 1,
        min_timeout_seconds: TEST_MIN_TIMEOUT_SECONDS,
    })
    .await
    .expect_err("timeout");
    assert_eq!(err.to_string(), "confirmation_wait_timeout");
}
