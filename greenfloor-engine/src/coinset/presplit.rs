use chia_protocol::Bytes32;
use chia_puzzle_types::cat::CatArgs;
use chia_sdk_coinset::{ChiaRpcClient, GetCoinRecordResponse};

use super::backend::OfferInputCatLookup;
use super::poll::{run_poll_loop, PollConfig};
use super::{cats, CoinsetClient};
use crate::coinset::retry::with_coinset_client_retries;
use crate::error::{SignerError, SignerResult};
use chia_sdk_driver::Cat;

const PRESPLIT_CONFIRM_TIMEOUT_SECS: u64 = 120;
const PRESPLIT_POLL_INTERVAL_SECS: u64 = 2;

/// Fetch an unspent offer-input CAT using an explicit lookup strategy.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn fetch_offer_input_cat(
    client: &CoinsetClient,
    lookup: OfferInputCatLookup,
) -> SignerResult<Cat> {
    match lookup {
        OfferInputCatLookup::ByCoinId(coin_id) => {
            fetch_unspent_offer_input_cat_by_id(client, coin_id).await
        }
        OfferInputCatLookup::ByCatFingerprint {
            asset_id,
            inner_puzzle_hash,
            amount,
        } => {
            fetch_unspent_offer_input_cat_by_inner_puzzle(
                client,
                asset_id,
                inner_puzzle_hash,
                amount,
            )
            .await
        }
    }
}

async fn fetch_unspent_offer_input_cat_by_id(
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
    if record.spent_block_index != 0 {
        return Err(SignerError::PresplitCoinNotFound);
    }
    cats::cat_from_record(client, &record)
        .await?
        .ok_or(SignerError::PresplitCoinNotFound)
}

async fn fetch_unspent_offer_input_cat_by_inner_puzzle(
    client: &CoinsetClient,
    asset_id: Bytes32,
    inner_puzzle_hash: Bytes32,
    amount: u64,
) -> SignerResult<Cat> {
    let cat_outer_puzzle_hash = CatArgs::curry_tree_hash(asset_id, inner_puzzle_hash.into()).into();
    let response = with_coinset_client_retries(|| async {
        client
            .get_coin_records_by_puzzle_hash(cat_outer_puzzle_hash, None, None, Some(false), None)
            .await
    })
    .await?;
    let records = cats::coin_records_from_response(response)?;
    for record in cats::unspent_coin_records(records) {
        if record.coin.amount != amount {
            continue;
        }
        let Some(cat) = cats::cat_from_record(client, &record).await? else {
            continue;
        };
        if cat.info.p2_puzzle_hash == inner_puzzle_hash && cat.coin.amount == amount {
            return Ok(cat);
        }
    }
    Err(SignerError::PresplitCoinNotFound)
}

/// Wait for unspent cat.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn wait_for_unspent_cat(client: &CoinsetClient, coin_id: Bytes32) -> SignerResult<Cat> {
    wait_for_unspent_cat_with_fetch(
        |coin_id| async move {
            let response: GetCoinRecordResponse = client
                .get_coin_record_by_name(coin_id)
                .await
                .map_err(SignerError::from)?;
            let Some(record) = response.coin_record else {
                return Ok(None);
            };
            if record.spent_block_index != 0 {
                return Ok(None);
            }
            cats::cat_from_record(client, &record).await
        },
        coin_id,
        PollConfig::from_seconds(PRESPLIT_CONFIRM_TIMEOUT_SECS, PRESPLIT_POLL_INTERVAL_SECS),
    )
    .await
}

pub(crate) async fn wait_for_unspent_cat_with_fetch<F, Fut>(
    mut fetch: F,
    coin_id: Bytes32,
    poll: PollConfig,
) -> SignerResult<Cat>
where
    F: FnMut(Bytes32) -> Fut,
    Fut: std::future::Future<Output = SignerResult<Option<Cat>>>,
{
    run_poll_loop(
        move || fetch(coin_id),
        poll,
        SignerError::PresplitCoinConfirmationTimeout,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::coinset::test_support::cat_with_amount;

    #[test]
    fn presplit_confirm_timeout_constants_are_sane() {
        const {
            assert!(PRESPLIT_CONFIRM_TIMEOUT_SECS >= PRESPLIT_POLL_INTERVAL_SECS);
        }
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
            PollConfig {
                timeout: Duration::from_millis(50),
                interval: Duration::from_millis(1),
            },
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
            PollConfig {
                timeout: Duration::from_millis(5),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SignerError::PresplitCoinConfirmationTimeout));
    }
}
