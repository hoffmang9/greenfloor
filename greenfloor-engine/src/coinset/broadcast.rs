use chia_protocol::SpendBundle;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use chia_traits::Streamable;

use super::rpc_result::ensure_coinset_success;
use crate::error::SignerResult;
use crate::hex::canonical_tx_id;

#[derive(Debug, Clone)]
pub struct BroadcastSpendBundleResult {
    pub status: String,
    pub operation_id: String,
}

/// Canonical operation id (spend-bundle hash) for a cancel/broadcast spend.
///
/// # Errors
///
/// Returns an error if the hash is not a valid 64-hex tx id.
pub fn spend_bundle_operation_id(spend_bundle: &SpendBundle) -> SignerResult<String> {
    canonical_tx_id(&hex::encode(spend_bundle.hash())).ok_or_else(|| {
        crate::error::SignerError::Other(
            "spend bundle hash did not produce a valid tx id".to_string(),
        )
    })
}

/// Broadcast spend bundle.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn broadcast_spend_bundle(
    client: &CoinsetClient,
    spend_bundle: SpendBundle,
) -> SignerResult<BroadcastSpendBundleResult> {
    let operation_id = spend_bundle_operation_id(&spend_bundle)?;
    // Coinset RPC expects structured SpendBundle JSON (not a hex string).
    let response = client
        .push_tx(spend_bundle)
        .await
        .map_err(crate::error::SignerError::from)?;
    ensure_coinset_success(
        response.success,
        response.error.as_deref(),
        "push_tx failed",
    )?;
    Ok(BroadcastSpendBundleResult {
        status: response.status,
        operation_id,
    })
}
