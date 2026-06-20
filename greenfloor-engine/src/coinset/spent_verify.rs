use chia_protocol::Bytes32;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use std::time::Duration;

use super::poll::{run_poll_loop, PollConfig};
use crate::error::{SignerError, SignerResult};

const DEFAULT_VERIFY_TIMEOUT_SECS: u64 = 15 * 60;
const DEFAULT_VERIFY_POLL_SECS: u64 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinSpentVerifyConfig {
    pub timeout_seconds: u64,
    pub poll_seconds: u64,
}

impl Default for CoinSpentVerifyConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: DEFAULT_VERIFY_TIMEOUT_SECS,
            poll_seconds: DEFAULT_VERIFY_POLL_SECS,
        }
    }
}

impl CoinSpentVerifyConfig {
    fn poll_config(self) -> PollConfig {
        #[cfg(test)]
        if self.timeout_seconds == 0 {
            return PollConfig {
                timeout: Duration::from_millis(10),
                interval: Duration::from_millis(1),
            };
        }
        PollConfig::from_seconds(self.timeout_seconds, self.poll_seconds)
    }
}

pub(crate) fn coin_record_is_spent(spent_block_index: u32) -> bool {
    spent_block_index != 0
}

async fn coin_is_spent(client: &CoinsetClient, coin_id: Bytes32) -> SignerResult<bool> {
    let response = client
        .get_coin_record_by_name(coin_id)
        .await
        .map_err(SignerError::from)?;
    Ok(response
        .coin_record
        .is_some_and(|record| coin_record_is_spent(record.spent_block_index)))
}

async fn all_coins_spent<F, Fut>(coin_ids: &[Bytes32], is_spent: &mut F) -> SignerResult<bool>
where
    F: FnMut(Bytes32) -> Fut,
    Fut: std::future::Future<Output = SignerResult<bool>>,
{
    for &coin_id in coin_ids {
        if !is_spent(coin_id).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Injectable spent-check hook for tests. Uses the same poll timing as [`run_poll_loop`]
/// because the checker is `FnMut` and cannot be re-entered from a poll attempt closure.
pub(crate) async fn wait_until_coins_spent_with_check<F, Fut>(
    mut is_spent: F,
    coin_ids: &[Bytes32],
    config: CoinSpentVerifyConfig,
) -> SignerResult<()>
where
    F: FnMut(Bytes32) -> Fut,
    Fut: std::future::Future<Output = SignerResult<bool>>,
{
    if coin_ids.is_empty() {
        return Ok(());
    }
    let poll = config.poll_config();
    let started = std::time::Instant::now();
    loop {
        if all_coins_spent(coin_ids, &mut is_spent).await? {
            return Ok(());
        }
        if started.elapsed() >= poll.timeout {
            return Err(SignerError::CombineInputVerifyTimeout);
        }
        tokio::time::sleep(poll.interval).await;
    }
}

/// Wait until coins spent.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn wait_until_coins_spent(
    client: &CoinsetClient,
    coin_ids: &[Bytes32],
    config: CoinSpentVerifyConfig,
) -> SignerResult<()> {
    if coin_ids.is_empty() {
        return Ok(());
    }
    let client = client.clone();
    let coin_ids = coin_ids.to_vec();
    run_poll_loop(
        move || {
            let client = client.clone();
            let coin_ids = coin_ids.clone();
            async move {
                for &coin_id in &coin_ids {
                    if !coin_is_spent(&client, coin_id).await? {
                        return Ok(None);
                    }
                }
                Ok(Some(()))
            }
        },
        config.poll_config(),
        SignerError::CombineInputVerifyTimeout,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_until_coins_spent_succeeds_when_all_inputs_marked_spent() {
        let coin_a = Bytes32::new([0x01; 32]);
        let coin_b = Bytes32::new([0x02; 32]);
        wait_until_coins_spent_with_check(
            |coin_id| async move { Ok(coin_id == coin_a || coin_id == coin_b) },
            &[coin_a, coin_b],
            CoinSpentVerifyConfig {
                timeout_seconds: 5,
                poll_seconds: 1,
            },
        )
        .await
        .expect("all spent");
    }

    #[tokio::test]
    async fn wait_until_coins_spent_times_out_when_input_stays_unspent() {
        let coin_id = Bytes32::new([0x03; 32]);
        let err = wait_until_coins_spent_with_check(
            |_| async { Ok(false) },
            std::slice::from_ref(&coin_id),
            CoinSpentVerifyConfig {
                timeout_seconds: 0,
                poll_seconds: 0,
            },
        )
        .await
        .expect_err("timeout");
        assert!(matches!(err, SignerError::CombineInputVerifyTimeout));
    }

    #[test]
    fn coin_record_is_spent_matches_spent_block_index_semantics() {
        assert!(!coin_record_is_spent(0));
        assert!(coin_record_is_spent(1));
    }
}
