use std::collections::HashSet;

use serde_json::{json, Value};

use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::SignerConfig;
use crate::cycle::retry::{poll_exponential_advance_sleep, poll_exponential_sleep_now};
use crate::error::{SignerError, SignerResult};

pub(super) struct BootstrapWaitConfig<'a> {
    pub network: &'a str,
    pub signer: &'a SignerConfig,
    pub receive_address: &'a str,
    pub asset_id: &'a str,
    pub initial_coin_ids: &'a HashSet<String>,
    pub timeout_seconds: u64,
    pub min_timeout_seconds: u64,
}

pub(super) async fn wait_for_coinset_confirmation(
    config: BootstrapWaitConfig<'_>,
) -> SignerResult<Vec<Value>> {
    let BootstrapWaitConfig {
        network,
        signer,
        receive_address,
        asset_id,
        initial_coin_ids,
        timeout_seconds,
        min_timeout_seconds,
    } = config;
    let start = std::time::Instant::now();
    let timeout = crate::config::u64_to_i64(
        timeout_seconds.max(min_timeout_seconds.max(1)),
        "runtime.offer_bootstrap_wait_timeout_seconds",
    )?;
    let initial_sleep = 2.0f64;
    let max_sleep = 20.0f64;
    let mut sleep_seconds = 0.0f64;
    loop {
        let elapsed_seconds = i64::try_from(start.elapsed().as_secs()).map_err(|_| {
            SignerError::Other("confirmation wait elapsed seconds overflow".to_string())
        })?;
        let Some(next_sleep) = poll_exponential_sleep_now(
            elapsed_seconds,
            timeout,
            sleep_seconds,
            initial_sleep,
            max_sleep,
        ) else {
            return Err(SignerError::Other("confirmation_wait_timeout".to_string()));
        };
        let coins =
            list_wallet_unspent_coins_for_signer(network, signer, receive_address, asset_id)
                .await?;
        let new_confirmed: Vec<_> = coins
            .into_iter()
            .filter(|coin| !initial_coin_ids.contains(&coin.id))
            .collect();
        if let Some(first) = new_confirmed.first() {
            return Ok(vec![json!({
                "event": "confirmed",
                "coin_name": first.name,
                "elapsed_seconds": elapsed_seconds.to_string(),
            })]);
        }
        tokio::time::sleep(std::time::Duration::from_secs_f64(next_sleep)).await;
        sleep_seconds =
            poll_exponential_advance_sleep(sleep_seconds, initial_sleep, max_sleep, 1.5);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::Value;

    use super::*;
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
}
