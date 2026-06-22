use chia_protocol::SpendBundle;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use chia_traits::Streamable;

use super::parse::ensure_coinset_typed_rpc_success;
use crate::error::SignerResult;

#[derive(Debug, Clone)]
pub struct BroadcastSpendBundleResult {
    pub status: String,
    pub operation_id: String,
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
    let operation_id = format!("0x{}", hex::encode(spend_bundle.hash()));
    // Coinset RPC expects structured SpendBundle JSON (not a hex string).
    let response = client
        .push_tx(spend_bundle)
        .await
        .map_err(crate::error::SignerError::from)?;
    ensure_coinset_typed_rpc_success(&response, "push_tx failed")?;
    Ok(BroadcastSpendBundleResult {
        status: response.status,
        operation_id,
    })
}
