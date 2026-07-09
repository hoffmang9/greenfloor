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
use crate::storage::SqliteStore;
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
    pub success: bool,
    pub operation_id: String,
    pub error: String,
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

#[derive(Debug, Clone)]
pub struct CancelOfferOnChainResult {
    pub operation_id: String,
}

/// Optional Dexie fallback for offer-file text when Coinset cannot supply it.
///
/// Coinset `get_offer` intentionally omits the raw offer blob, so this is only
/// used for legacy / Dexie-posted rows that still need the `offer1…` string.
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
    let coinset_client =
        client_for_signer_on_network(&params.signer_config, params.operator_network)?;
    let backend = LiveCoinset(&coinset_client);
    let mut vault_ctx = resolve_vault_spend_context(params.signer_config.clone()).await?;

    let spend_bundle = if let Some(text) = params.offer_text.as_deref() {
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
    } else if let Some(text) =
        resolve_offer_file_text_for_cancel(params.offer_id, None, params.dexie).await?
    {
        build_offer_cancel_spend_bundle(
            &mut vault_ctx,
            &backend,
            &text,
            params.cancel_metadata.as_ref(),
        )
        .await?
    } else {
        return Err(SignerError::Other(
            "offer cancel requires local offer file, stored cancel metadata, or Dexie offer-file fallback"
                .to_string(),
        ));
    };
    let operation_id = spend_bundle_operation_id(&spend_bundle)?;
    Ok((spend_bundle, operation_id))
}

fn restore_offer_after_failed_cancel_broadcast(
    store: &SqliteStore,
    offer_id: &str,
    market_id: &str,
    prior_state: &str,
    cancel_metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<()> {
    store.unchecked_transaction_scope("cancel_broadcast_rollback", |store| {
        store.upsert_offer_state(offer_id, market_id, prior_state, None)?;
        if let Some(meta) = cancel_metadata {
            let mut coins = Vec::new();
            let mut p2s = Vec::new();
            if let Some(coin) = meta.fields.input_coin_id.as_ref() {
                coins.push(coin.clone());
            }
            if let Some(p2) = meta.fields.fixed_delegated_puzzle_hash.as_ref() {
                p2s.push(p2.clone());
            }
            if !coins.is_empty() || !p2s.is_empty() {
                store.replace_offer_coin_watches_no_txn(offer_id, market_id, &coins, &p2s)?;
            }
        }
        Ok(())
    })
}

/// Cancel an offer on-chain by spending an offered input coin back to vault change.
///
/// Prefer order:
/// 1. Explicit local `offer_text` (CLI `--offer-file`)
/// 2. Coinset coin lookup + stored cancel metadata (no offer blob required)
/// 3. Optional Dexie offer-file fetch (legacy / Dexie venue only)
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn cancel_offer_on_chain(
    params: CancelOfferOnChainParams<'_>,
) -> SignerResult<CancelOfferOnChainResult> {
    let (spend_bundle, _) = build_cancel_spend_bundle(&params).await?;
    let coinset_client =
        client_for_signer_on_network(&params.signer_config, params.operator_network)?;
    let broadcast = coinset::broadcast_spend_bundle(&coinset_client, spend_bundle).await?;
    Ok(CancelOfferOnChainResult {
        operation_id: broadcast.operation_id,
    })
}

fn failure_outcome(
    target: &CancelOfferTarget,
    market_id: String,
    operation_id: String,
    error: impl Into<String>,
) -> CancelOfferOutcome {
    CancelOfferOutcome {
        offer_id: target.offer_id().to_string(),
        market_id,
        success: false,
        operation_id,
        error: error.into(),
    }
}

fn success_outcome(
    target: &CancelOfferTarget,
    market_id: String,
    operation_id: String,
) -> CancelOfferOutcome {
    CancelOfferOutcome {
        offer_id: target.offer_id().to_string(),
        market_id,
        success: true,
        operation_id,
        error: String::new(),
    }
}

fn prior_offer_state(store: &SqliteStore, offer_id: &str) -> SignerResult<Option<String>> {
    Ok(store
        .list_offer_states_for_ids(std::slice::from_ref(&offer_id.to_string()))?
        .into_iter()
        .next()
        .map(|row| row.state))
}

fn persist_cancel_submitted_before_broadcast(
    store: &SqliteStore,
    target: &CancelOfferTarget,
    market_id: &str,
    operation_id: &str,
) -> Result<(), CancelOfferOutcome> {
    if !target.persists_state() {
        return Ok(());
    }
    store
        .upsert_offer_cancel_submitted(target.offer_id(), market_id, operation_id, None)
        .map_err(|err| {
            failure_outcome(
                target,
                market_id.to_string(),
                String::new(),
                format!("cancel_submitted persist failed before broadcast: {err}"),
            )
        })
}

fn rollback_tracked_cancel(
    store: &SqliteStore,
    target: &CancelOfferTarget,
    market_id: &str,
    prior_state: Option<&str>,
    cancel_metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<()> {
    if !target.persists_state() {
        return Ok(());
    }
    restore_offer_after_failed_cancel_broadcast(
        store,
        target.offer_id(),
        market_id,
        prior_state.unwrap_or("open"),
        cancel_metadata,
    )
}

struct BroadcastCancelSpend<'a> {
    store: &'a SqliteStore,
    signer_config: &'a SignerConfig,
    operator_network: &'a str,
    target: &'a CancelOfferTarget,
    market_id: String,
    spend_bundle: SpendBundle,
    operation_id: String,
    prior_state: Option<String>,
    cancel_metadata: Option<&'a StoredOfferCancelMetadata>,
}

async fn broadcast_cancel_spend(args: BroadcastCancelSpend<'_>) -> CancelOfferOutcome {
    let BroadcastCancelSpend {
        store,
        signer_config,
        operator_network,
        target,
        market_id,
        spend_bundle,
        operation_id,
        prior_state,
        cancel_metadata,
    } = args;
    let coinset_client = match client_for_signer_on_network(signer_config, operator_network) {
        Ok(client) => client,
        Err(err) => {
            let _ = rollback_tracked_cancel(
                store,
                target,
                &market_id,
                prior_state.as_deref(),
                cancel_metadata,
            );
            return failure_outcome(target, market_id, operation_id, err.to_string());
        }
    };
    match coinset::broadcast_spend_bundle(&coinset_client, spend_bundle).await {
        Ok(result) => success_outcome(target, market_id, result.operation_id),
        Err(err) => {
            if let Err(rollback_err) = rollback_tracked_cancel(
                store,
                target,
                &market_id,
                prior_state.as_deref(),
                cancel_metadata,
            ) {
                return failure_outcome(
                    target,
                    market_id,
                    operation_id,
                    format!(
                        "cancel broadcast failed ({err}); rollback also failed: {rollback_err}"
                    ),
                );
            }
            failure_outcome(target, market_id, operation_id, err.to_string())
        }
    }
}

async fn cancel_one_offer(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    signer_config: &SignerConfig,
    operator_network: &str,
    target: &CancelOfferTarget,
) -> SignerResult<CancelOfferOutcome> {
    let market_id = target.normalized_market_id();
    let cancel_metadata = if target.persists_state() {
        store.offer_cancel_metadata_for_id(target.offer_id())?
    } else {
        None
    };
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
        cancel_metadata: cancel_metadata.clone(),
    };
    let (spend_bundle, operation_id) = match build_cancel_spend_bundle(&params).await {
        Ok(value) => value,
        Err(err) => {
            return Ok(failure_outcome(
                target,
                market_id,
                String::new(),
                err.to_string(),
            ));
        }
    };
    let prior_state = if target.persists_state() {
        prior_offer_state(store, target.offer_id())?
    } else {
        None
    };
    if let Err(outcome) =
        persist_cancel_submitted_before_broadcast(store, target, &market_id, &operation_id)
    {
        return Ok(outcome);
    }
    Ok(broadcast_cancel_spend(BroadcastCancelSpend {
        store,
        signer_config,
        operator_network,
        target,
        market_id,
        spend_bundle,
        operation_id,
        prior_state,
        cancel_metadata: cancel_metadata.as_ref(),
    })
    .await)
}

/// Cancel offers on-chain (spend an offered input coin back to vault change).
///
/// Cancellation is submitted via Coinset. Offer-file text is optional: Coinset +
/// stored cancel metadata is preferred; Dexie is only an optional offer-file fallback.
/// Tracked cancels persist `cancel_submitted` (and clear watches) **before** broadcast
/// using the deterministic spend-bundle hash as the cancel tx id; broadcast failure
/// rolls state back.
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
        if target.offer_text().is_some() {
            continue;
        }
        if !target.persists_state() {
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
            Some("open"),
            "failed tracked cancel must not advance lifecycle state"
        );
    }
}
