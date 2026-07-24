//! On-chain offer cancel: build reclaim spend, prepare/broadcast, observe or roll back.

mod broadcast;
mod build;
mod target;

#[cfg(test)]
mod tests;

use crate::adapters::DexieClient;
use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::offer::types::StoredOfferCancelMetadata;
use crate::storage::SqliteStore;

use broadcast::{broadcast_cancel, CancelPersistPolicy};
use build::{build_cancel_spend_bundle, needs_dexie_offer_file};
use target::failed;

pub use target::{CancelOfferOutcome, CancelOfferTarget};

/// Tracked-path context loaded once from [`CancelOfferTarget::Tracked`].
struct TrackedCancelCtx {
    metadata: Option<StoredOfferCancelMetadata>,
}

async fn cancel_one_offer(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    signer_config: &SignerConfig,
    operator_network: &str,
    target: &CancelOfferTarget,
) -> SignerResult<CancelOfferOutcome> {
    let market_id = target.normalized_market_id();
    let tracked = match target {
        CancelOfferTarget::Tracked { offer_id, .. } => Some(TrackedCancelCtx {
            metadata: store.offer_cancel_metadata_for_id(offer_id)?,
        }),
        CancelOfferTarget::LocalFile { .. } => None,
    };

    let (spend_bundle, operation_id, coinset_client) = match build_cancel_spend_bundle(
        signer_config,
        operator_network,
        target.offer_id(),
        target.offer_text(),
        tracked.as_ref().and_then(|ctx| ctx.metadata.as_ref()),
        dexie,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => return Ok(failed(target, market_id, "", err.to_string())),
    };

    let persist = match tracked {
        Some(_) => {
            let prior_state = store.offer_state_for_id(target.offer_id())?;
            if let Err(err) = store.prepare_offer_cancel_submitted(
                target.offer_id(),
                &market_id,
                &operation_id,
                None,
            ) {
                return Ok(failed(
                    target,
                    market_id,
                    "",
                    format!("cancel_submitted prepare failed before broadcast: {err}"),
                ));
            }
            CancelPersistPolicy::Tracked { store, prior_state }
        }
        None => CancelPersistPolicy::Ephemeral,
    };

    Ok(broadcast_cancel(
        target,
        &market_id,
        &coinset_client,
        spend_bundle,
        operation_id,
        persist,
    )
    .await)
}

/// Cancel offers on-chain (spend an offered input coin back to vault change).
///
/// Tracked cancels: prepare `cancel_submitted` (state + tx id, watches kept) →
/// `push_tx` → observe cancel tx (watches kept until terminal) on success, or roll
/// state back on broadcast failure.
///
/// # Failure model
///
/// Per-target orchestration failures (build, prepare, broadcast, observe, rollback)
/// are returned as [`CancelOfferOutcome::Failed`] so the batch can continue.
/// Infrastructure failures that prevent evaluating a target (for example
/// `SQLite` metadata/state reads before soft handling) propagate as `Err`.
///
/// # Errors
///
/// Returns an error if infrastructure required to evaluate a target fails.
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
        if !target.persists_state() {
            continue;
        }
        let metadata = store.offer_cancel_metadata_for_id(target.offer_id())?;
        if needs_dexie_offer_file(target.offer_text(), metadata.as_ref()) {
            return Ok(true);
        }
    }
    Ok(false)
}
