use chia_protocol::SpendBundle;
use chia_traits::Streamable;
use serde_json::{json, Value};

use super::rpc::direct_coinset_client;
use crate::coinset::broadcast_spend_bundle;
use crate::error::{SignerError, SignerResult};

/// Push tx hex.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn push_tx_hex(
    network: &str,
    base_url: Option<&str>,
    spend_bundle_hex: &str,
) -> SignerResult<Value> {
    let client = direct_coinset_client(network, base_url)?;
    let raw = spend_bundle_hex.trim().trim_start_matches("0x");
    let bytes =
        hex::decode(raw).map_err(|err| SignerError::Other(format!("invalid hex: {err}")))?;
    let spend_bundle = SpendBundle::from_bytes(&bytes)
        .map_err(|err: chia_traits::Error| SignerError::Other(err.to_string()))?;
    let result = broadcast_spend_bundle(&client, spend_bundle).await?;
    Ok(json!({
        "success": true,
        "status": result.status,
        "operation_id": result.operation_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::SpendBundle;

    #[tokio::test]
    async fn push_tx_hex_returns_success_payload() {
        let bundle = SpendBundle::new(Vec::new(), chia_bls::Signature::default());
        let spend_bundle_hex = hex::encode(
            bundle
                .to_bytes()
                .expect("serialize empty spend bundle for push tx test"),
        );

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/push_tx")
            .with_status(200)
            .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
            .create_async()
            .await;

        let result = push_tx_hex("mainnet", Some(&server.url()), &spend_bundle_hex)
            .await
            .expect("push tx");
        assert_eq!(result.get("success").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result.get("status").and_then(|value| value.as_str()),
            Some("SUCCESS")
        );
    }
}
