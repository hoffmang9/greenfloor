use crate::adapters::DexieClient;
use crate::coinset::{self, client_for_signer_on_network, spend_bundle_operation_id, LiveCoinset};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::cancel_input::metadata_sufficient_for_coinset_cancel;
use crate::offer::dexie_payload::DexieOfferPayload;
use crate::offer::reclaim::{
    build_offer_cancel_spend_bundle, build_offer_cancel_spend_bundle_from_metadata,
};
use crate::offer::types::StoredOfferCancelMetadata;
use crate::storage::{SqliteStore, TxSignalIngress};
use crate::vault::session::resolve_vault_spend_context;
use chia_protocol::SpendBundle;

/// Cancel target for on-chain offer reclaim.
#[derive(Debug, Clone)]
pub enum CancelOfferTarget {
    /// Tracked offer id; lifecycle state is updated on successful submit.
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
    /// True when the cancel spend was broadcast successfully.
    pub success: bool,
    pub operation_id: String,
    /// Hard failure (build/broadcast/rollback). Empty on success.
    pub error: String,
    /// Soft failure after successful broadcast (e.g. observe cancel tx failed).
    pub warning: String,
}

#[derive(Debug, Clone)]
pub struct CancelOfferOnChainParams<'a> {
    pub offer_id: &'a str,
    pub signer_config: SignerConfig,
    pub operator_network: &'a str,
    /// Optional Dexie client used only when Coinset/local metadata cannot supply
    /// the offer file (legacy Dexie-posted rows without usable cancel metadata).
    pub dexie: Option<&'a DexieClient>,
    pub fee_mojos: u64,
    pub offer_text: Option<String>,
    pub cancel_metadata: Option<StoredOfferCancelMetadata>,
}

fn outcome(
    target: &CancelOfferTarget,
    market_id: String,
    success: bool,
    operation_id: String,
    error: impl Into<String>,
) -> CancelOfferOutcome {
    CancelOfferOutcome {
        offer_id: target.offer_id().to_string(),
        market_id,
        success,
        operation_id,
        error: error.into(),
        warning: String::new(),
    }
}

fn outcome_with_warning(
    target: &CancelOfferTarget,
    market_id: String,
    operation_id: String,
    warning: impl Into<String>,
) -> CancelOfferOutcome {
    CancelOfferOutcome {
        offer_id: target.offer_id().to_string(),
        market_id,
        success: true,
        operation_id,
        error: String::new(),
        warning: warning.into(),
    }
}

async fn fetch_optional_dexie_offer_file_text(
    dexie: &DexieClient,
    offer_id: &str,
) -> SignerResult<String> {
    let response = dexie.get_offer(offer_id).await?;
    if response.is_explicit_failure() {
        return Err(SignerError::OfferCancelOfferFileNotFound);
    }
    let payload = DexieOfferPayload::new(response.into_value());
    payload
        .offer_file_text()
        .map(str::to_string)
        .ok_or(SignerError::OfferCancelOfferFileMissing)
}

async fn resolve_offer_file_text_for_cancel(
    offer_id: &str,
    offer_text: Option<String>,
    dexie: Option<&DexieClient>,
) -> SignerResult<Option<String>> {
    if let Some(text) = offer_text {
        return Ok(Some(text));
    }
    let Some(dexie) = dexie else {
        return Ok(None);
    };
    Ok(Some(
        fetch_optional_dexie_offer_file_text(dexie, offer_id).await?,
    ))
}

async fn build_cancel_spend_bundle(
    params: &CancelOfferOnChainParams<'_>,
) -> SignerResult<(SpendBundle, String)> {
    if params.fee_mojos > 0 {
        return Err(SignerError::Other(
            "offer cancel fee not supported yet".to_string(),
        ));
    }
    // Resolve offer inputs before vault/KMS so Dexie/local failures surface without
    // requiring signer credentials.
    let offer_text = if let Some(text) = params.offer_text.clone() {
        Some(text)
    } else if metadata_sufficient_for_coinset_cancel(params.cancel_metadata.as_ref()) {
        None
    } else {
        resolve_offer_file_text_for_cancel(params.offer_id, None, params.dexie).await?
    };
    let coinset_client =
        client_for_signer_on_network(&params.signer_config, params.operator_network)?;
    let backend = LiveCoinset(&coinset_client);
    let mut vault_ctx = resolve_vault_spend_context(params.signer_config.clone()).await?;

    let spend_bundle = if let Some(text) = offer_text.as_deref() {
        build_offer_cancel_spend_bundle(
            &mut vault_ctx,
            &backend,
            text,
            params.cancel_metadata.as_ref(),
        )
        .await?
    } else if let Some(metadata) = params
        .cancel_metadata
        .as_ref()
        .filter(|meta| metadata_sufficient_for_coinset_cancel(Some(meta)))
    {
        build_offer_cancel_spend_bundle_from_metadata(&mut vault_ctx, &backend, metadata).await?
    } else {
        return Err(SignerError::Other(
            "offer cancel requires local offer file, stored cancel metadata, or Dexie offer-file fallback"
                .to_string(),
        ));
    };
    let operation_id = spend_bundle_operation_id(&spend_bundle)?;
    Ok((spend_bundle, operation_id))
}

fn prior_offer_state(store: &SqliteStore, offer_id: &str) -> SignerResult<Option<String>> {
    Ok(store
        .list_offer_states_for_ids(std::slice::from_ref(&offer_id.to_string()))?
        .into_iter()
        .next()
        .map(|row| row.state))
}

async fn broadcast_and_settle_tracked_cancel(
    store: &SqliteStore,
    target: &CancelOfferTarget,
    market_id: &str,
    prior_state: Option<&str>,
    coinset_client: &chia_sdk_coinset::CoinsetClient,
    spend_bundle: SpendBundle,
    operation_id: String,
) -> CancelOfferOutcome {
    match coinset::broadcast_spend_bundle(coinset_client, spend_bundle).await {
        Ok(result) => {
            match store.ingest_tx_signals(
                std::slice::from_ref(&result.operation_id),
                TxSignalIngress::Mempool,
            ) {
                Ok(_) => outcome(
                    target,
                    market_id.to_string(),
                    true,
                    result.operation_id,
                    String::new(),
                ),
                Err(err) => outcome_with_warning(
                    target,
                    market_id.to_string(),
                    result.operation_id,
                    format!("cancel broadcast succeeded; observe cancel tx failed: {err}"),
                ),
            }
        }
        Err(err) => {
            if let Err(rollback_err) = store.rollback_offer_cancel_submitted(
                target.offer_id(),
                market_id,
                prior_state.unwrap_or("open"),
            ) {
                return outcome(
                    target,
                    market_id.to_string(),
                    false,
                    operation_id,
                    format!(
                        "cancel broadcast failed ({err}); rollback also failed: {rollback_err}"
                    ),
                );
            }
            outcome(
                target,
                market_id.to_string(),
                false,
                operation_id,
                err.to_string(),
            )
        }
    }
}

/// Prepare tracked cancel state. Returns `Some(failure outcome)` on error.
fn prepare_tracked_cancel_or_outcome(
    store: &SqliteStore,
    target: &CancelOfferTarget,
    market_id: &str,
    operation_id: &str,
) -> Option<CancelOfferOutcome> {
    store
        .prepare_offer_cancel_submitted(target.offer_id(), market_id, operation_id, None)
        .err()
        .map(|err| {
            outcome(
                target,
                market_id.to_string(),
                false,
                String::new(),
                format!("cancel_submitted prepare failed before broadcast: {err}"),
            )
        })
}

async fn build_cancel_bundle_for_target(
    dexie: Option<&DexieClient>,
    signer_config: &SignerConfig,
    operator_network: &str,
    target: &CancelOfferTarget,
    cancel_metadata: Option<StoredOfferCancelMetadata>,
) -> SignerResult<(SpendBundle, String, String)> {
    let market_id = target.normalized_market_id();
    let dexie_for_target = if target.offer_text().is_some()
        || metadata_sufficient_for_coinset_cancel(cancel_metadata.as_ref())
    {
        None
    } else {
        dexie
    };
    let params = CancelOfferOnChainParams {
        offer_id: target.offer_id(),
        signer_config: signer_config.clone(),
        operator_network,
        dexie: dexie_for_target,
        fee_mojos: 0,
        offer_text: target.offer_text().map(str::to_string),
        cancel_metadata,
    };
    let (spend_bundle, operation_id) = build_cancel_spend_bundle(&params).await?;
    Ok((spend_bundle, operation_id, market_id))
}

async fn cancel_tracked_offer(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    signer_config: &SignerConfig,
    operator_network: &str,
    target: &CancelOfferTarget,
) -> SignerResult<CancelOfferOutcome> {
    let cancel_metadata = store.offer_cancel_metadata_for_id(target.offer_id())?;
    let (spend_bundle, operation_id, market_id) = match build_cancel_bundle_for_target(
        dexie,
        signer_config,
        operator_network,
        target,
        cancel_metadata,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            return Ok(outcome(
                target,
                target.normalized_market_id(),
                false,
                String::new(),
                err.to_string(),
            ));
        }
    };
    let prior_state = prior_offer_state(store, target.offer_id())?;
    if let Some(out) = prepare_tracked_cancel_or_outcome(store, target, &market_id, &operation_id) {
        return Ok(out);
    }
    let coinset_client = match client_for_signer_on_network(signer_config, operator_network) {
        Ok(client) => client,
        Err(err) => {
            let _ = store.rollback_offer_cancel_submitted(
                target.offer_id(),
                &market_id,
                prior_state.as_deref().unwrap_or("open"),
            );
            return Ok(outcome(
                target,
                market_id,
                false,
                operation_id,
                err.to_string(),
            ));
        }
    };
    Ok(broadcast_and_settle_tracked_cancel(
        store,
        target,
        &market_id,
        prior_state.as_deref(),
        &coinset_client,
        spend_bundle,
        operation_id,
    )
    .await)
}

async fn cancel_local_file_offer(
    _store: &SqliteStore,
    dexie: Option<&DexieClient>,
    signer_config: &SignerConfig,
    operator_network: &str,
    target: &CancelOfferTarget,
) -> SignerResult<CancelOfferOutcome> {
    let (spend_bundle, operation_id, market_id) =
        match build_cancel_bundle_for_target(dexie, signer_config, operator_network, target, None)
            .await
        {
            Ok(value) => value,
            Err(err) => {
                return Ok(outcome(
                    target,
                    target.normalized_market_id(),
                    false,
                    String::new(),
                    err.to_string(),
                ));
            }
        };
    let coinset_client = match client_for_signer_on_network(signer_config, operator_network) {
        Ok(client) => client,
        Err(err) => {
            return Ok(outcome(
                target,
                market_id,
                false,
                operation_id,
                err.to_string(),
            ));
        }
    };
    match coinset::broadcast_spend_bundle(&coinset_client, spend_bundle).await {
        Ok(result) => Ok(outcome(
            target,
            market_id,
            true,
            result.operation_id,
            String::new(),
        )),
        Err(err) => Ok(outcome(
            target,
            market_id,
            false,
            operation_id,
            err.to_string(),
        )),
    }
}

async fn cancel_one_offer(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    signer_config: &SignerConfig,
    operator_network: &str,
    target: &CancelOfferTarget,
) -> SignerResult<CancelOfferOutcome> {
    match target {
        CancelOfferTarget::Tracked { .. } => {
            cancel_tracked_offer(store, dexie, signer_config, operator_network, target).await
        }
        CancelOfferTarget::LocalFile { .. } => {
            cancel_local_file_offer(store, dexie, signer_config, operator_network, target).await
        }
    }
}

/// Cancel offers on-chain (spend an offered input coin back to vault change).
///
/// Tracked cancels: prepare `cancel_submitted` (state + tx id, watches kept) →
/// `push_tx` → observe cancel tx (watches kept until terminal) on success, or roll
/// state back on broadcast failure.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn cancel_offers_on_chain(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    signer_config: SignerConfig,
    operator_network: &str,
    targets: &[CancelOfferTarget],
) -> SignerResult<Vec<CancelOfferOutcome>> {
    let mut outcomes = Vec::with_capacity(targets.len());
    for target in targets {
        outcomes
            .push(cancel_one_offer(store, dexie, &signer_config, operator_network, target).await?);
    }
    Ok(outcomes)
}

/// Whether any tracked target needs Dexie offer-file fallback (no local text, incomplete metadata).
///
/// # Errors
///
/// Returns an error if cancel-metadata reads fail.
pub fn cancel_targets_need_dexie_fallback(
    store: &SqliteStore,
    targets: &[CancelOfferTarget],
) -> SignerResult<bool> {
    for target in targets {
        if target.offer_text().is_some() || !target.persists_state() {
            continue;
        }
        let metadata = store.offer_cancel_metadata_for_id(target.offer_id())?;
        if !metadata_sufficient_for_coinset_cancel(metadata.as_ref()) {
            return Ok(true);
        }
    }
    Ok(false)
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

    #[test]
    fn prepare_keeps_watches_and_observe_does_not_clear() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        let coin = "11".repeat(32);
        let p2 = "22".repeat(32);
        let cancel_tx = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("seed");
        store
            .replace_offer_coin_watches(
                &offer_id,
                "m1",
                std::slice::from_ref(&coin),
                std::slice::from_ref(&p2),
            )
            .expect("watches");
        store
            .prepare_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
            .expect("prepare");
        let (coins, p2s) = store
            .list_offer_coin_watches_for_offer(&offer_id)
            .expect("still watched");
        assert_eq!(coins, vec![coin.clone()]);
        assert_eq!(p2s, vec![p2]);
        store
            .ingest_tx_signals(std::slice::from_ref(&cancel_tx), TxSignalIngress::Mempool)
            .expect("observe");
        assert_eq!(
            store
                .list_offer_ids_for_watched_coin(&coin)
                .expect("watches kept after observe"),
            vec![offer_id.clone()]
        );
        let signals = store
            .get_tx_signal_state(std::slice::from_ref(&cancel_tx))
            .expect("tx");
        assert!(signals
            .get(&cancel_tx)
            .is_some_and(|row| row.mempool_observed_at.is_some()));
    }

    #[test]
    fn rollback_prepare_restores_prior_state_without_touching_watches() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let offer_id = "ab".repeat(32);
        let coin = "11".repeat(32);
        let cancel_tx = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("seed");
        store
            .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("watches");
        store
            .prepare_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
            .expect("prepare");
        store
            .rollback_offer_cancel_submitted(&offer_id, "m1", "open")
            .expect("rollback");
        assert_eq!(
            store
                .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
                .expect("rows")[0]
                .state,
            "open"
        );
        assert_eq!(
            store
                .list_offer_ids_for_watched_coin(&coin)
                .expect("watches kept"),
            vec![offer_id]
        );
        assert!(!store
            .get_tx_signal_state(std::slice::from_ref(&cancel_tx))
            .expect("no observe yet")
            .contains_key(&cancel_tx));
    }

    #[tokio::test]
    async fn local_file_cancel_does_not_write_offer_state() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        store
            .upsert_offer_state("existing-offer", "m1", "open", Some(0))
            .expect("seed");
        let target = CancelOfferTarget::LocalFile {
            offer_id: "local-offer-test".to_string(),
            market_id: "m1".to_string(),
            offer_text: "offer1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(),
        };
        let outcomes = cancel_offers_on_chain(
            &store,
            None,
            test_signer_config("http://127.0.0.1:1"),
            "mainnet",
            std::slice::from_ref(&target),
        )
        .await
        .expect("cancel batch");
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].success);
        assert!(store
            .offer_state_for_id("local-offer-test")
            .expect("lookup")
            .is_none());
        assert_eq!(
            store
                .offer_state_for_id("existing-offer")
                .expect("lookup")
                .as_deref(),
            Some("open")
        );
    }

    #[tokio::test]
    async fn tracked_cancel_failure_does_not_write_cancel_submitted() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        store
            .upsert_offer_state("offer-open", "m1", "open", Some(0))
            .expect("seed");
        let target = CancelOfferTarget::Tracked {
            offer_id: "offer-open".to_string(),
            market_id: "m1".to_string(),
        };
        let outcomes = cancel_offers_on_chain(
            &store,
            None,
            test_signer_config("http://127.0.0.1:1"),
            "mainnet",
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
            Some("open")
        );
    }
}
