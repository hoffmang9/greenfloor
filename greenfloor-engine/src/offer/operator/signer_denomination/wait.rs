use std::collections::HashSet;

use serde_json::{json, Value};

use crate::coinset::list_wallet_unspent_coins;
use crate::cycle::retry::{poll_exponential_advance_sleep, poll_exponential_sleep_now};
use crate::error::{SignerError, SignerResult};

pub(super) async fn wait_for_coinset_confirmation(
    network: &str,
    receive_address: &str,
    asset_id: &str,
    initial_coin_ids: &HashSet<String>,
    timeout_seconds: u64,
    msp_base_url: Option<&str>,
) -> SignerResult<Vec<Value>> {
    let start = std::time::Instant::now();
    let min_timeout = if cfg!(test) { 1 } else { 10 };
    let timeout = crate::config::u64_to_i64(
        timeout_seconds.max(min_timeout),
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
            list_wallet_unspent_coins(network, receive_address, asset_id, msp_base_url).await?;
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

    const RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

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

        let events = wait_for_coinset_confirmation(
            "mainnet",
            RECEIVE_ADDRESS,
            "xch",
            &HashSet::new(),
            5,
            Some(&server.url()),
        )
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

        let err = wait_for_coinset_confirmation(
            "mainnet",
            RECEIVE_ADDRESS,
            "xch",
            &HashSet::new(),
            1,
            Some(&server.url()),
        )
        .await
        .expect_err("timeout");
        assert_eq!(err.to_string(), "confirmation_wait_timeout");
    }
}
