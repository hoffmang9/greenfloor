use crate::adapters::DexieClient;
use crate::coinset::{self, client_for_signer, LiveCoinset};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::dexie_payload::DexieOfferPayload;
use crate::offer::reclaim::build_offer_cancel_spend_bundle;
use crate::offer::types::StoredOfferCancelMetadata;
use crate::storage::SqliteStore;
use crate::vault::session::resolve_vault_spend_context;
const CANCEL_SUBMIT_PERSIST_RETRIES: u32 = 2;

fn persist_cancel_submitted_after_broadcast(
    store: &SqliteStore,
    offer_id: &str,
    market_id: &str,
    cancel_tx_id: &str,
    last_seen_status: Option<i64>,
) -> SignerResult<()> {
    let mut last_err = None;
    for _ in 0..CANCEL_SUBMIT_PERSIST_RETRIES {
        match store.upsert_offer_cancel_submitted(
            offer_id,
            market_id,
            cancel_tx_id,
            last_seen_status,
        ) {
            Ok(()) => return Ok(()),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        SignerError::Other(
            "cancel broadcast succeeded but cancel_submitted persist failed after retries"
                .to_string(),
        )
    }))
}

/// Cancel target for on-chain offer reclaim.
#[derive(Debug, Clone)]
pub enum CancelOfferTarget {
    /// Dexie-tracked offer id; lifecycle state is updated on successful submit.
    Tracked { offer_id: String, market_id: String },
    /// Local offer file or bech32; cancel spends without `SQLite` lifecycle updates.
    LocalFile {
        offer_id: String,
        market_id: String,
        offer_text: String,
    },
}

impl CancelOfferTarget {
    #[must_use]
    pub fn offer_id(&self) -> &str {
        match self {
            Self::Tracked { offer_id, .. } | Self::LocalFile { offer_id, .. } => offer_id,
        }
    }

    #[must_use]
    pub fn market_id(&self) -> &str {
        match self {
            Self::Tracked { market_id, .. } | Self::LocalFile { market_id, .. } => market_id,
        }
    }

    #[must_use]
    pub fn normalized_market_id(&self) -> String {
        let market_id = self.market_id().trim();
        if market_id.is_empty() {
            "unknown".to_string()
        } else {
            market_id.to_string()
        }
    }

    #[must_use]
    pub fn offer_text(&self) -> Option<&str> {
        match self {
            Self::Tracked { .. } => None,
            Self::LocalFile { offer_text, .. } => Some(offer_text.as_str()),
        }
    }

    #[must_use]
    pub fn persists_state(&self) -> bool {
        matches!(self, Self::Tracked { .. })
    }
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
    pub cancel_metadata: Option<StoredOfferCancelMetadata>,
}

#[derive(Debug, Clone)]
pub struct CancelOfferOnChainResult {
    pub operation_id: String,
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
    let coinset_client = client_for_signer(&params.signer_config)?;
    let backend = LiveCoinset(&coinset_client);
    let mut vault_ctx = resolve_vault_spend_context(params.signer_config).await?;
    let spend_bundle = build_offer_cancel_spend_bundle(
        &mut vault_ctx,
        &backend,
        &offer_text,
        params.cancel_metadata.as_ref(),
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
        let market_id = target.normalized_market_id();
        let cancel_metadata = if target.persists_state() {
            store.offer_cancel_metadata_for_id(target.offer_id())?
        } else {
            None
        };
        match cancel_offer_on_chain(CancelOfferOnChainParams {
            offer_id: target.offer_id(),
            signer_config: signer_config.clone(),
            dexie,
            fee_mojos: 0,
            offer_text: target.offer_text().map(str::to_string),
            cancel_metadata,
        })
        .await
        {
            Ok(result) => {
                let mut error = String::new();
                if target.persists_state() {
                    if let Err(err) = persist_cancel_submitted_after_broadcast(
                        store,
                        target.offer_id(),
                        &market_id,
                        &result.operation_id,
                        None,
                    ) {
                        error = format!(
                            "cancel broadcast succeeded (tx {}) but state persist failed: {err}",
                            result.operation_id
                        );
                    }
                }
                outcomes.push(CancelOfferOutcome {
                    offer_id: target.offer_id().to_string(),
                    market_id,
                    success: error.is_empty(),
                    operation_id: result.operation_id,
                    error,
                });
            }
            Err(err) => {
                outcomes.push(CancelOfferOutcome {
                    offer_id: target.offer_id().to_string(),
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
    use tempfile::tempdir;

    use super::*;
    use crate::test_support::signer_config::test_signer_config;

    #[test]
    fn tracked_target_persists_state() {
        let target = CancelOfferTarget::Tracked {
            offer_id: "offer-1".to_string(),
            market_id: "m1".to_string(),
        };
        assert_eq!(target.market_id(), "m1");
        assert!(target.persists_state());
        assert!(target.offer_text().is_none());
    }

    #[test]
    fn local_file_target_is_ephemeral() {
        let target = CancelOfferTarget::LocalFile {
            offer_id: "local-offer-1".to_string(),
            market_id: "m1".to_string(),
            offer_text: "offer1qqq".to_string(),
        };
        assert!(!target.persists_state());
        assert_eq!(target.offer_text(), Some("offer1qqq"));
    }

    #[tokio::test]
    async fn local_file_cancel_does_not_write_offer_state() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("existing-offer", "m1", "open", Some(0))
            .expect("seed");
        let target = CancelOfferTarget::LocalFile {
            offer_id: "local-offer-test".to_string(),
            market_id: "m1".to_string(),
            offer_text: "offer1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(),
        };
        let dexie = DexieClient::new("http://127.0.0.1:1");
        let outcomes = cancel_offers_on_chain(
            &store,
            &dexie,
            test_signer_config("http://127.0.0.1:1"),
            std::slice::from_ref(&target),
        )
        .await
        .expect("cancel batch");
        assert_eq!(outcomes.len(), 1);
        assert!(
            !outcomes[0].success,
            "invalid offer should fail before broadcast"
        );
        assert!(
            store
                .offer_state_for_id("local-offer-test")
                .expect("lookup")
                .is_none(),
            "local file cancel must not upsert offer_state"
        );
        assert_eq!(
            store
                .offer_state_for_id("existing-offer")
                .expect("lookup")
                .as_deref(),
            Some("open"),
            "unrelated offer rows must be preserved"
        );
    }

    #[tokio::test]
    async fn tracked_cancel_failure_does_not_write_cancel_submitted() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-open", "m1", "open", Some(0))
            .expect("seed");
        let target = CancelOfferTarget::Tracked {
            offer_id: "offer-open".to_string(),
            market_id: "m1".to_string(),
        };
        let dexie = DexieClient::new("http://127.0.0.1:1");
        let outcomes = cancel_offers_on_chain(
            &store,
            &dexie,
            test_signer_config("http://127.0.0.1:1"),
            std::slice::from_ref(&target),
        )
        .await
        .expect("cancel batch");
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].success);
        assert_eq!(
            store
                .offer_state_for_id("offer-open")
                .expect("lookup")
                .as_deref(),
            Some("open"),
            "failed tracked cancel must not advance lifecycle state"
        );
    }
}
