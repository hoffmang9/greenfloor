use crate::adapters::{DexieClient, DexieResponse};
use crate::cycle::{dexie_invalid_offer_retry_sleep, dexie_invalid_offer_should_retry};
use crate::error::{OfferError, SignerResult};
use crate::offer::lifecycle::reconcile_prep::{fetch_dexie_offer, DexieOfferFetch};

use super::{dexie_offer_asset_expectation_error, ExpectedPublishAssetFields};

const DEXIE_INVALID_OFFER_RETRY_MAX_ATTEMPTS: u32 = 4;
const DEXIE_INVALID_OFFER_RETRY_INITIAL_SLEEP_SECONDS: f64 = 1.0;
const DEXIE_VISIBILITY_POLL_ATTEMPTS: u32 = 4;
const DEXIE_VISIBILITY_POLL_DELAY_SECONDS: f64 = 1.5;
const DEXIE_VISIBILITY_REPOST_MAX_ATTEMPTS: u32 = 3;
const DEXIE_VISIBILITY_REPOST_DELAY_SECONDS: f64 = 2.0;

#[derive(Debug, Clone)]
pub struct PostOfferPhaseDexieParams<'a> {
    pub dexie: &'a DexieClient,
    pub offer_text: &'a str,
    pub drop_only: bool,
    pub claim_rewards: bool,
    pub expected: &'a ExpectedPublishAssetFields,
}

enum DexiePostVisibility {
    Visible,
    Missing,
}

async fn sleep_for_publish(seconds: f64) {
    // Unit tests validate poll/repost behavior by attempt counts and mock expectations,
    // not wall-clock delays. Under `cfg(test)` sleeps are no-ops so lib tests stay fast.
    #[cfg(not(test))]
    tokio::time::sleep(std::time::Duration::from_secs_f64(seconds)).await;
    #[cfg(test)]
    {
        let _ = seconds;
        tokio::task::yield_now().await;
    }
}

async fn post_dexie_offer_with_invalid_offer_retry(
    dexie: &DexieClient,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
) -> SignerResult<DexieResponse> {
    let mut attempt = 0u32;
    loop {
        match dexie.post_offer(offer_text, drop_only, claim_rewards).await {
            Ok(result) => return Ok(result),
            Err(err)
                if dexie_invalid_offer_should_retry(
                    &err,
                    attempt,
                    DEXIE_INVALID_OFFER_RETRY_MAX_ATTEMPTS,
                ) =>
            {
                let sleep_seconds = dexie_invalid_offer_retry_sleep(
                    attempt,
                    DEXIE_INVALID_OFFER_RETRY_INITIAL_SLEEP_SECONDS,
                );
                sleep_for_publish(sleep_seconds).await;
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

async fn wait_for_dexie_offer_visible(
    dexie: &DexieClient,
    offer_id: &str,
    expected: &ExpectedPublishAssetFields,
) -> SignerResult<DexiePostVisibility> {
    let clean_offer_id = offer_id.trim();
    if clean_offer_id.is_empty() {
        return Err(OfferError::DexieOfferMissingIdAfterPublish.into());
    }
    let mut last = DexieOfferFetch::Mismatch;
    let mut last_err = None;
    for attempt in 1..=DEXIE_VISIBILITY_POLL_ATTEMPTS {
        match fetch_dexie_offer(dexie, clean_offer_id).await {
            Ok(DexieOfferFetch::Found(offer_obj)) => {
                if let Some(asset_error) = dexie_offer_asset_expectation_error(
                    offer_obj.get("offered").unwrap_or(&serde_json::Value::Null),
                    offer_obj
                        .get("requested")
                        .unwrap_or(&serde_json::Value::Null),
                    expected,
                ) {
                    return Err(OfferError::DexieOfferAssetMismatch(asset_error).into());
                }
                return Ok(DexiePostVisibility::Visible);
            }
            Ok(fetch) => {
                last = fetch;
                last_err = None;
            }
            Err(err) => last_err = Some(err),
        }
        if attempt < DEXIE_VISIBILITY_POLL_ATTEMPTS {
            sleep_for_publish(DEXIE_VISIBILITY_POLL_DELAY_SECONDS).await;
        }
    }
    if let Some(err) = last_err {
        return Err(err);
    }
    match last {
        DexieOfferFetch::Missing => Ok(DexiePostVisibility::Missing),
        DexieOfferFetch::Found(_) | DexieOfferFetch::Mismatch => {
            Err(OfferError::DexieOfferVisibilityMismatch.into())
        }
    }
}

/// Post offer to Dexie with invalid-offer retry and post-publish visibility checks.
///
/// # Errors
///
/// Returns an error if the post, visibility poll, or asset check fails.
pub async fn post_offer_phase_dexie(
    params: PostOfferPhaseDexieParams<'_>,
) -> SignerResult<DexieResponse> {
    let PostOfferPhaseDexieParams {
        dexie,
        offer_text,
        drop_only,
        claim_rewards,
        expected,
    } = params;
    for attempt in 1..=DEXIE_VISIBILITY_REPOST_MAX_ATTEMPTS {
        let result =
            post_dexie_offer_with_invalid_offer_retry(dexie, offer_text, drop_only, claim_rewards)
                .await?;
        if !result.success() {
            return Ok(result);
        }
        let posted_offer_id = result.offer_id().unwrap_or("").to_string();
        match wait_for_dexie_offer_visible(dexie, &posted_offer_id, expected).await? {
            DexiePostVisibility::Visible => return Ok(result),
            DexiePostVisibility::Missing => {
                if attempt < DEXIE_VISIBILITY_REPOST_MAX_ATTEMPTS {
                    sleep_for_publish(DEXIE_VISIBILITY_REPOST_DELAY_SECONDS).await;
                }
            }
        }
    }
    Err(OfferError::DexieOfferNotVisible.into())
}

#[cfg(test)]
mod tests;
