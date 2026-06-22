use crate::adapters::DexieClient;
use crate::coinset::{self, client_for_config, LiveCoinset};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::dexie_payload::DexieOfferPayload;
use crate::offer::reclaim::build_offer_cancel_spend_bundle;
use crate::offer::types::PresplitCancelFields;
use crate::storage::SqliteStore;
use crate::vault::session::resolve_vault_spend_context;

#[derive(Debug, Clone)]
pub struct CancelOfferTarget {
    pub offer_id: String,
    pub market_id: String,
    pub state: String,
    /// When set, skip Dexie fetch and cancel from this offer file text directly.
    pub offer_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CancelOfferOutcome {
    pub offer_id: String,
    pub market_id: String,
    pub success: bool,
    pub operation_id: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct CancelOfferOnChainParams<'a> {
    pub offer_id: &'a str,
    pub signer_config: SignerConfig,
    pub dexie: &'a DexieClient,
    pub fee_mojos: u64,
    pub offer_text: Option<String>,
    pub cancel_fields: Option<PresplitCancelFields>,
}

#[derive(Debug, Clone)]
pub struct CancelOfferOnChainResult {
    pub operation_id: String,
}

fn normalize_cancel_tx_id(operation_id: &str) -> String {
    normalize_hex_id(operation_id)
}

async fn fetch_dexie_offer_text(dexie: &DexieClient, offer_id: &str) -> SignerResult<String> {
    let response = dexie.get_offer(offer_id).await?;
    if response.is_explicit_failure() {
        return Err(SignerError::OfferCancelDexieOfferNotFound);
    }
    let payload = DexieOfferPayload::new(response.into_value());
    payload
        .offer_file_text()
        .map(str::to_string)
        .ok_or(SignerError::OfferCancelOfferFileMissing)
}

/// Cancel an offer on-chain by spending an offered input coin back to vault change.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn cancel_offer_on_chain(
    params: CancelOfferOnChainParams<'_>,
) -> SignerResult<CancelOfferOnChainResult> {
    if params.fee_mojos > 0 {
        return Err(SignerError::Other(
            "offer cancel fee not supported yet".to_string(),
        ));
    }
    let offer_text = if let Some(text) = params.offer_text {
        text
    } else {
        fetch_dexie_offer_text(params.dexie, params.offer_id).await?
    };
    let coinset_client = client_for_config(&params.signer_config)?;
    let backend = LiveCoinset(&coinset_client);
    let mut vault_ctx = resolve_vault_spend_context(params.signer_config).await?;
    let spend_bundle = build_offer_cancel_spend_bundle(
        &mut vault_ctx,
        &backend,
        &offer_text,
        params.cancel_fields.as_ref(),
    )
    .await?;
    let broadcast = coinset::broadcast_spend_bundle(&coinset_client, spend_bundle).await?;
    Ok(CancelOfferOnChainResult {
        operation_id: broadcast.operation_id,
    })
}

/// Cancel offers on-chain (spend an offered input coin back to vault change).
///
/// Dexie is used only to fetch the offer file text; cancellation is submitted via Coinset.
/// Successful submits record `cancel_submitted` and observe the cancel tx for reconcile.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn cancel_offers_on_chain(
    store: &SqliteStore,
    dexie: &DexieClient,
    signer_config: SignerConfig,
    targets: &[CancelOfferTarget],
) -> SignerResult<Vec<CancelOfferOutcome>> {
    let mut outcomes = Vec::with_capacity(targets.len());
    for target in targets {
        let market_id = if target.market_id.trim().is_empty() {
            "unknown".to_string()
        } else {
            target.market_id.clone()
        };
        let cancel_fields = store.offer_cancel_fields_for_id(&target.offer_id)?;
        match cancel_offer_on_chain(CancelOfferOnChainParams {
            offer_id: &target.offer_id,
            signer_config: signer_config.clone(),
            dexie,
            fee_mojos: 0,
            offer_text: target.offer_text.clone(),
            cancel_fields,
        })
        .await
        {
            Ok(result) => {
                let tx_id = normalize_cancel_tx_id(&result.operation_id);
                if !tx_id.is_empty() {
                    store.observe_mempool_tx_ids(std::slice::from_ref(&tx_id))?;
                }
                store.upsert_offer_state(&target.offer_id, &market_id, "cancel_submitted", None)?;
                outcomes.push(CancelOfferOutcome {
                    offer_id: target.offer_id.clone(),
                    market_id,
                    success: true,
                    operation_id: result.operation_id,
                    error: String::new(),
                });
            }
            Err(err) => {
                outcomes.push(CancelOfferOutcome {
                    offer_id: target.offer_id.clone(),
                    market_id,
                    success: false,
                    operation_id: String::new(),
                    error: err.to_string(),
                });
            }
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::CancelOfferTarget;

    #[test]
    fn cancel_target_carries_market_and_state() {
        let target = CancelOfferTarget {
            offer_id: "offer-1".to_string(),
            market_id: "m1".to_string(),
            state: "open".to_string(),
            offer_text: None,
        };
        assert_eq!(target.market_id, "m1");
        assert_eq!(target.state, "open");
    }
}
