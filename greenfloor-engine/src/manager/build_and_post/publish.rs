use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use crate::adapters::{dexie_offer_view_url, post_offer_phase_dexie, DexieClient, SplashClient};
use crate::error::{SignerError, SignerResult};
use crate::storage::{
    persist_offer_post_records, state_db_path_for_home, OfferPostPersistRecord, SqliteStore,
};

use super::context::ResolvedBuildAndPostContext;
use super::types::PublishResult;

pub(super) async fn publish_offer(
    publish_venue: &str,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
    offer_text: &str,
    drop_only: bool,
    claim_rewards: bool,
    expected_offered_asset_id: &str,
    expected_offered_symbol: &str,
    expected_requested_asset_id: &str,
    expected_requested_symbol: &str,
) -> SignerResult<PublishResult> {
    let body = match publish_venue {
        "dexie" => {
            let dexie = dexie.ok_or_else(|| {
                SignerError::Other("dexie adapter missing for dexie publish".to_string())
            })?;
            post_offer_phase_dexie(
                dexie,
                offer_text,
                drop_only,
                claim_rewards,
                expected_offered_asset_id,
                expected_offered_symbol,
                expected_requested_asset_id,
                expected_requested_symbol,
            )
            .await?
        }
        "splash" => {
            let splash = splash.ok_or_else(|| {
                SignerError::Other("splash adapter missing for splash publish".to_string())
            })?;
            splash.post_offer(offer_text).await?
        }
        other => {
            return Err(SignerError::Other(format!(
                "unsupported publish venue: {other}"
            )));
        }
    };
    Ok(PublishResult::from_adapter_body(body))
}

pub(super) fn finalize_publish_payload(
    publish: PublishResult,
    execution_mode: &str,
    timing_ms: Value,
    dexie_base_url: Option<&str>,
) -> Value {
    let mut payload = publish.body;
    if let Value::Object(obj) = &mut payload {
        obj.insert("execution_mode".to_string(), json!(execution_mode));
        obj.insert("timing_ms".to_string(), timing_ms);
        if publish.success {
            if let (Some(base_url), Some(offer_id)) = (dexie_base_url, publish.offer_id.as_deref())
            {
                obj.insert(
                    "offer_view_url".to_string(),
                    Value::String(dexie_offer_view_url(base_url, offer_id)),
                );
            }
        }
    }
    payload
}

pub(super) fn offer_post_persist_record(
    publish: &PublishResult,
    side: &str,
    execution_mode: &str,
    ctx: &ResolvedBuildAndPostContext,
    size_base_units: u64,
) -> Option<OfferPostPersistRecord> {
    if !publish.success {
        return None;
    }
    let offer_id = publish.offer_id.clone()?;
    Some(OfferPostPersistRecord {
        offer_id,
        market_id: ctx.market.market_id.clone(),
        side: side.to_string(),
        size_base_units,
        publish_venue: ctx.publish_venue.clone(),
        resolved_base_asset_id: ctx.resolved_base_asset_id.clone(),
        resolved_quote_asset_id: ctx.resolved_quote_asset_id.clone(),
        created_extra: json!({"execution_mode": execution_mode}),
    })
}

pub fn persist_post_records_if_enabled(
    home_dir: &Path,
    persist_results: bool,
    dry_run: bool,
    records: &[OfferPostPersistRecord],
) -> SignerResult<()> {
    if !persist_results || dry_run || records.is_empty() {
        return Ok(());
    }
    let db_path = state_db_path_for_home(home_dir);
    let store = SqliteStore::open(&db_path)?;
    persist_offer_post_records(&store, records)
}
