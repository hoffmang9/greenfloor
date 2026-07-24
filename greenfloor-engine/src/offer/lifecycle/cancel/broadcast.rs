use crate::coinset;
use crate::storage::{SqliteStore, TxSignalIngress};
use chia_protocol::SpendBundle;

use super::target::{failed, submitted, CancelOfferOutcome, CancelOfferTarget};

pub(super) enum CancelPersistPolicy<'a> {
    Tracked {
        store: &'a SqliteStore,
        prior_state: Option<String>,
    },
    Ephemeral,
}

pub(super) async fn broadcast_cancel(
    target: &CancelOfferTarget,
    market_id: &str,
    coinset_client: &chia_sdk_coinset::CoinsetClient,
    spend_bundle: SpendBundle,
    operation_id: String,
    persist: CancelPersistPolicy<'_>,
) -> CancelOfferOutcome {
    match coinset::broadcast_spend_bundle(coinset_client, spend_bundle).await {
        Ok(result) => {
            if let CancelPersistPolicy::Tracked { store, .. } = persist {
                if let Err(err) = store.ingest_tx_signals(
                    std::slice::from_ref(&result.operation_id),
                    TxSignalIngress::Mempool,
                ) {
                    return failed(
                        target,
                        market_id,
                        result.operation_id,
                        format!("cancel broadcast succeeded; observe cancel tx failed: {err}"),
                    );
                }
            }
            submitted(target, market_id, result.operation_id)
        }
        Err(err) => {
            if let CancelPersistPolicy::Tracked { store, prior_state } = persist {
                if let Err(rollback_err) = store.rollback_offer_cancel_submitted(
                    target.offer_id(),
                    market_id,
                    prior_state.as_deref().unwrap_or("open"),
                ) {
                    return failed(
                        target,
                        market_id,
                        operation_id,
                        format!(
                            "cancel broadcast failed ({err}); rollback also failed: {rollback_err}"
                        ),
                    );
                }
            }
            failed(target, market_id, operation_id, err.to_string())
        }
    }
}
