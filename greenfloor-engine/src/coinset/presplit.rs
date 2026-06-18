use chia_protocol::Bytes32;
use chia_sdk_coinset::{ChiaRpcClient, GetCoinRecordResponse};

use super::{cat_from_record, CoinsetClient};
use crate::error::{SignerError, SignerResult};
use chia_sdk_driver::Cat;

const PRESPLIT_CONFIRM_TIMEOUT_SECS: u64 = 120;
const PRESPLIT_POLL_INTERVAL_SECS: u64 = 2;

pub(crate) fn presplit_confirm_timed_out(
    started: std::time::Instant,
    now: std::time::Instant,
) -> bool {
    now.duration_since(started).as_secs() >= PRESPLIT_CONFIRM_TIMEOUT_SECS
}

pub async fn fetch_presplit_cat_by_id(
    client: &CoinsetClient,
    coin_id: Bytes32,
) -> SignerResult<Cat> {
    let response = client
        .get_coin_record_by_name(coin_id)
        .await
        .map_err(SignerError::from)?;
    let Some(record) = response.coin_record else {
        return Err(SignerError::PresplitCoinNotFound);
    };
    if record.spent {
        return Err(SignerError::PresplitCoinNotFound);
    }
    cat_from_record(client, &record)
        .await?
        .ok_or(SignerError::PresplitCoinNotFound)
}

pub async fn wait_for_unspent_cat(client: &CoinsetClient, coin_id: Bytes32) -> SignerResult<Cat> {
    let started = std::time::Instant::now();
    wait_for_unspent_cat_with_fetch(
        |coin_id| async move {
            let response: GetCoinRecordResponse = client
                .get_coin_record_by_name(coin_id)
                .await
                .map_err(SignerError::from)?;
            let Some(record) = response.coin_record else {
                return Ok(None);
            };
            if record.spent {
                return Ok(None);
            }
            cat_from_record(client, &record).await
        },
        coin_id,
        started,
        std::time::Duration::from_secs(PRESPLIT_POLL_INTERVAL_SECS),
        presplit_confirm_timed_out,
    )
    .await
}

pub(crate) async fn wait_for_unspent_cat_with_fetch<F, Fut>(
    mut fetch: F,
    coin_id: Bytes32,
    started: std::time::Instant,
    poll_interval: std::time::Duration,
    timed_out: fn(std::time::Instant, std::time::Instant) -> bool,
) -> SignerResult<Cat>
where
    F: FnMut(Bytes32) -> Fut,
    Fut: std::future::Future<Output = SignerResult<Option<Cat>>>,
{
    loop {
        if let Some(cat) = fetch(coin_id).await? {
            return Ok(cat);
        }
        if timed_out(started, std::time::Instant::now()) {
            return Err(SignerError::PresplitCoinConfirmationTimeout);
        }
        tokio::time::sleep(poll_interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::{Bytes32, Coin};
    use chia_sdk_driver::{Cat, CatInfo};

    fn cat_with_amount(amount: u64) -> Cat {
        Cat::new(
            Coin::new(
                Bytes32::new([u8::try_from(amount).unwrap_or(0u8); 32]),
                Bytes32::default(),
                amount,
            ),
            None,
            CatInfo::new(Bytes32::new([0x01; 32]), None, Bytes32::default()),
        )
    }

    #[test]
    fn presplit_confirm_timeout_constants_are_sane() {
        const {
            assert!(PRESPLIT_CONFIRM_TIMEOUT_SECS >= PRESPLIT_POLL_INTERVAL_SECS);
        }
    }

    #[test]
    fn presplit_confirm_timed_out_after_timeout_window() {
        let started = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(
                PRESPLIT_CONFIRM_TIMEOUT_SECS + 1,
            ))
            .unwrap();
        assert!(presplit_confirm_timed_out(
            started,
            std::time::Instant::now()
        ));
    }

    #[tokio::test]
    async fn wait_for_unspent_cat_succeeds_after_delayed_availability() {
        let coin_id = Bytes32::new([0xab; 32]);
        let expected = cat_with_amount(1000);
        let attempts = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let cat = wait_for_unspent_cat_with_fetch(
            {
                let attempts = std::rc::Rc::clone(&attempts);
                move |_coin_id| {
                    attempts.set(attempts.get() + 1);
                    let expected = expected;
                    let attempts = std::rc::Rc::clone(&attempts);
                    async move {
                        if attempts.get() < 2 {
                            Ok(None)
                        } else {
                            Ok(Some(expected))
                        }
                    }
                }
            },
            coin_id,
            std::time::Instant::now(),
            std::time::Duration::from_millis(1),
            |started, now| now.duration_since(started) >= std::time::Duration::from_millis(50),
        )
        .await
        .expect("cat confirmed");
        assert_eq!(cat.coin.amount, 1000);
        assert_eq!(attempts.get(), 2);
    }

    #[tokio::test]
    async fn wait_for_unspent_cat_times_out_when_cat_never_appears() {
        let coin_id = Bytes32::new([0xcd; 32]);
        let err = wait_for_unspent_cat_with_fetch(
            |_coin_id| async { Ok(None) },
            coin_id,
            std::time::Instant::now(),
            std::time::Duration::from_millis(1),
            |started, now| now.duration_since(started) >= std::time::Duration::from_millis(5),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SignerError::PresplitCoinConfirmationTimeout));
    }
}
