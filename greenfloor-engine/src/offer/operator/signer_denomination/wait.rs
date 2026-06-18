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
) -> SignerResult<Vec<Value>> {
    let start = std::time::Instant::now();
    let timeout = crate::config::u64_to_i64(
        timeout_seconds.max(10),
        "runtime.offer_bootstrap_wait_timeout_seconds",
    )?;
    let initial_sleep = 2.0f64;
    let max_sleep = 20.0f64;
    let mut sleep_seconds = 0.0f64;
    loop {
        let elapsed_seconds = start.elapsed().as_secs().try_into().unwrap_or(0i64);
        let Some(next_sleep) = poll_exponential_sleep_now(
            elapsed_seconds,
            timeout,
            sleep_seconds,
            initial_sleep,
            max_sleep,
        ) else {
            return Err(SignerError::Other("confirmation_wait_timeout".to_string()));
        };
        let coins = list_wallet_unspent_coins(network, receive_address, asset_id).await?;
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
