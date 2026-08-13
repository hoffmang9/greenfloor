use serde_json::json;

use crate::adapters::{DexieClient, DexieResponse};
use crate::cycle::{dexie_invalid_offer_retry_sleep, dexie_invalid_offer_should_retry};
use crate::error::{SignerError, SignerResult, TransportError};
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

#[derive(Debug)]
enum OfferVisibilityWait {
    Visible,
    Failed(String),
    Missing(String),
    Unresolved(String),
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

fn dexie_publish_failure(response: DexieResponse, error: impl Into<String>) -> DexieResponse {
    let error = error.into();
    let offer_id = response.offer_id().map(str::to_string);
    let mut body = response.into_value();
    match &mut body {
        serde_json::Value::Object(obj) => {
            obj.insert("success".to_string(), serde_json::Value::Bool(false));
            obj.insert("error".to_string(), serde_json::Value::String(error));
        }
        _ => {
            body = json!({
                "success": false,
                "error": error,
                "id": offer_id,
            });
        }
    }
    DexieResponse::from_value(body)
}

fn failed_dexie_from_http_status(err: SignerError) -> SignerResult<DexieResponse> {
    if matches!(
        &err,
        SignerError::Transport(TransportError::HttpStatus { .. })
    ) {
        Ok(DexieResponse::from_value(json!({
            "success": false,
            "error": err.to_string(),
        })))
    } else {
        Err(err)
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
            Err(err) => return failed_dexie_from_http_status(err),
        }
    }
}

async fn wait_for_dexie_offer_visible(
    dexie: &DexieClient,
    offer_id: &str,
    expected: &ExpectedPublishAssetFields,
) -> OfferVisibilityWait {
    let clean_offer_id = offer_id.trim();
    if clean_offer_id.is_empty() {
        return OfferVisibilityWait::Unresolved("dexie_offer_missing_id_after_publish".to_string());
    }
    let mut last: Result<DexieOfferFetch, String> =
        Err("dexie_offer_not_visible_after_publish".to_string());
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
                    return OfferVisibilityWait::Failed(asset_error);
                }
                return OfferVisibilityWait::Visible;
            }
            Ok(fetch) => last = Ok(fetch),
            Err(err) => last = Err(format!("dexie_get_offer_error:{err}")),
        }
        if attempt < DEXIE_VISIBILITY_POLL_ATTEMPTS {
            sleep_for_publish(DEXIE_VISIBILITY_POLL_DELAY_SECONDS).await;
        }
    }
    match last {
        Ok(DexieOfferFetch::Missing(error)) => OfferVisibilityWait::Missing(error),
        Ok(_) => {
            OfferVisibilityWait::Unresolved("dexie_offer_visibility_payload_mismatch".to_string())
        }
        Err(error) => OfferVisibilityWait::Unresolved(error),
    }
}

/// Post offer to Dexie with invalid-offer retry and post-publish visibility checks.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
    let mut last_result = DexieResponse::from_value(json!({
        "success": false,
        "error": "dexie_offer_not_visible_after_publish",
    }));
    let mut last_missing_error = String::new();
    for attempt in 1..=DEXIE_VISIBILITY_REPOST_MAX_ATTEMPTS {
        let result =
            post_dexie_offer_with_invalid_offer_retry(dexie, offer_text, drop_only, claim_rewards)
                .await?;
        last_result = result.clone();
        if !result.success() {
            return Ok(result);
        }
        let posted_offer_id = result.offer_id().unwrap_or("").to_string();
        match wait_for_dexie_offer_visible(dexie, &posted_offer_id, expected).await {
            OfferVisibilityWait::Visible => return Ok(result),
            OfferVisibilityWait::Failed(error) | OfferVisibilityWait::Unresolved(error) => {
                return Ok(dexie_publish_failure(result, error));
            }
            OfferVisibilityWait::Missing(error) => {
                last_missing_error = error;
                if attempt < DEXIE_VISIBILITY_REPOST_MAX_ATTEMPTS {
                    sleep_for_publish(DEXIE_VISIBILITY_REPOST_DELAY_SECONDS).await;
                }
            }
        }
    }
    Ok(dexie_publish_failure(
        last_result,
        if last_missing_error.is_empty() {
            "dexie_offer_not_visible_after_publish".to_string()
        } else {
            last_missing_error
        },
    ))
}

#[cfg(test)]
mod tests;
