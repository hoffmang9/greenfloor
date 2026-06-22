use serde_json::{json, Value};

use super::rpc::post_msp_coinset_rpc;
use crate::error::SignerResult;

/// Get all mempool tx ids.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn get_all_mempool_tx_ids(
    network: &str,
    base_url: Option<&str>,
) -> SignerResult<Vec<String>> {
    let payload =
        post_msp_coinset_rpc(network, base_url, "get_all_mempool_tx_ids", json!({})).await?;
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(Vec::new());
    }
    let tx_ids = payload
        .get("tx_ids")
        .or_else(|| payload.get("mempool_tx_ids"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(tx_ids
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_all_mempool_tx_ids_via_msp_client() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"tx_ids":["0xabc"]}"#)
            .create_async()
            .await;

        let tx_ids = get_all_mempool_tx_ids("mainnet", Some(&server.url()))
            .await
            .expect("mempool tx ids");
        assert_eq!(tx_ids, vec!["0xabc".to_string()]);
    }
}
