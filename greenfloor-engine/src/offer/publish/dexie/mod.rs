use serde_json::json;

use crate::adapters::{DexieClient, DexieResponse};
use crate::cycle::{
    dexie_invalid_offer_retry_sleep, dexie_invalid_offer_should_retry,
    is_transient_dexie_visibility_404_error,
};
use crate::error::SignerResult;

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
pub(super) enum OfferVisibilityPoll {
    Ready,
    Retry(String),
    Failed(String),
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

async fn post_dexie_offer_with_invalid_offer_retry(
    dexie: &DexieClient,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
) -> SignerResult<DexieResponse> {
    let mut attempt = 0u32;
    loop {
        let result = dexie
            .post_offer(offer_text, drop_only, claim_rewards)
            .await?;
        if !dexie_invalid_offer_should_retry(
            result.error_text(),
            attempt,
            DEXIE_INVALID_OFFER_RETRY_MAX_ATTEMPTS,
        ) {
            return Ok(result);
        }
        let sleep_seconds = dexie_invalid_offer_retry_sleep(
            attempt,
            DEXIE_INVALID_OFFER_RETRY_INITIAL_SLEEP_SECONDS,
        );
        sleep_for_publish(sleep_seconds).await;
        attempt += 1;
    }
}

pub(super) async fn poll_dexie_offer_visibility_once(
    dexie: &DexieClient,
    offer_id: &str,
    expected: &ExpectedPublishAssetFields,
) -> OfferVisibilityPoll {
    let payload = match dexie.get_offer(offer_id).await {
        Ok(payload) => payload,
        Err(err) => {
            return OfferVisibilityPoll::Retry(format!("dexie_get_offer_error:{err}"));
        }
    };
    if payload.is_explicit_failure() {
        let error = if payload.error_text().is_empty() {
            "dexie_offer_not_visible_after_publish".to_string()
        } else {
            payload.error_text().to_string()
        };
        return OfferVisibilityPoll::Retry(error);
    }
    let offer_payload = payload.offer_payload();
    let visible_id = offer_payload
        .and_then(serde_json::Value::as_object)
        .and_then(|obj| obj.get("id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if visible_id != offer_id {
        return OfferVisibilityPoll::Retry("dexie_offer_visibility_payload_mismatch".to_string());
    }
    if let Some(offer_obj) = offer_payload.and_then(serde_json::Value::as_object) {
        if let Some(asset_error) = dexie_offer_asset_expectation_error(
            offer_obj.get("offered").unwrap_or(&serde_json::Value::Null),
            offer_obj
                .get("requested")
                .unwrap_or(&serde_json::Value::Null),
            expected,
        ) {
            return OfferVisibilityPoll::Failed(asset_error);
        }
    }
    OfferVisibilityPoll::Ready
}

async fn wait_for_dexie_offer_visible(
    dexie: &DexieClient,
    offer_id: &str,
    expected: &ExpectedPublishAssetFields,
) -> Option<String> {
    let clean_offer_id = offer_id.trim();
    if clean_offer_id.is_empty() {
        return Some("dexie_offer_missing_id_after_publish".to_string());
    }
    let mut last_error = "dexie_offer_not_visible_after_publish".to_string();
    for attempt in 1..=DEXIE_VISIBILITY_POLL_ATTEMPTS {
        match poll_dexie_offer_visibility_once(dexie, clean_offer_id, expected).await {
            OfferVisibilityPoll::Ready => return None,
            OfferVisibilityPoll::Failed(error) => return Some(error),
            OfferVisibilityPoll::Retry(error) => {
                last_error = error;
                if attempt < DEXIE_VISIBILITY_POLL_ATTEMPTS {
                    sleep_for_publish(DEXIE_VISIBILITY_POLL_DELAY_SECONDS).await;
                }
            }
        }
    }
    Some(last_error)
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
    let mut last_visibility_error = String::new();
    for attempt in 1..=DEXIE_VISIBILITY_REPOST_MAX_ATTEMPTS {
        let result =
            post_dexie_offer_with_invalid_offer_retry(dexie, offer_text, drop_only, claim_rewards)
                .await?;
        last_result = result.clone();
        if !result.success() {
            return Ok(result);
        }
        let posted_offer_id = result.offer_id().unwrap_or("").to_string();
        if let Some(visibility_error) =
            wait_for_dexie_offer_visible(dexie, &posted_offer_id, expected).await
        {
            last_visibility_error = visibility_error;
            if !is_transient_dexie_visibility_404_error(&last_visibility_error) {
                return Ok(dexie_publish_failure(result, last_visibility_error));
            }
            if attempt < DEXIE_VISIBILITY_REPOST_MAX_ATTEMPTS {
                sleep_for_publish(DEXIE_VISIBILITY_REPOST_DELAY_SECONDS).await;
            }
            continue;
        }
        return Ok(result);
    }
    Ok(dexie_publish_failure(
        last_result,
        if last_visibility_error.is_empty() {
            "dexie_offer_not_visible_after_publish".to_string()
        } else {
            last_visibility_error
        },
    ))
}

#[cfg(test)]
mod tests;
