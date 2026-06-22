use serde_json::{json, Value};

use super::rpc::post_msp_coinset_rpc;
use crate::error::SignerResult;

/// Get fee estimate.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn get_fee_estimate(
    network: &str,
    base_url: Option<&str>,
    target_times: Vec<u64>,
    cost: u64,
    spend_count: Option<u64>,
) -> SignerResult<Value> {
    let mut body = json!({
        "target_times": target_times,
        "cost": cost.max(1),
    });
    if let Some(count) = spend_count.filter(|value| *value > 0) {
        body["spend_count"] = json!(count);
    }
    post_msp_coinset_rpc(network, base_url, "get_fee_estimate", body).await
}

pub fn conservative_fee_from_payload(payload: &Value) -> Option<u64> {
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    if let Some(estimates) = payload.get("estimates").and_then(Value::as_array) {
        let mut valid = Vec::new();
        for value in estimates {
            if let Some(parsed) = value.as_u64() {
                valid.push(parsed);
            } else if let Some(parsed) = value.as_i64().filter(|v| *v >= 0) {
                if let Ok(parsed_u64) = u64::try_from(parsed) {
                    valid.push(parsed_u64);
                }
            }
        }
        if !valid.is_empty() {
            return Some(*valid.iter().max()?);
        }
    }
    payload.get("fee_estimate").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
    })
}

/// Get conservative fee estimate.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn get_conservative_fee_estimate(
    network: &str,
    base_url: Option<&str>,
    cost: u64,
    spend_count: Option<u64>,
) -> SignerResult<Option<u64>> {
    let payload =
        get_fee_estimate(network, base_url, vec![300, 600, 1200], cost, spend_count).await?;
    Ok(conservative_fee_from_payload(&payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conservative_fee_uses_max_estimate() {
        let payload = json!({"success": true, "estimates": [100, 500, 200]});
        assert_eq!(conservative_fee_from_payload(&payload), Some(500));
    }

    #[test]
    fn conservative_fee_falls_back_to_fee_estimate_field() {
        let payload = json!({"success": true, "fee_estimate": 42});
        assert_eq!(conservative_fee_from_payload(&payload), Some(42));
    }

    #[test]
    fn conservative_fee_returns_none_on_failure() {
        let payload = json!({"success": false});
        assert_eq!(conservative_fee_from_payload(&payload), None);
    }

    #[tokio::test]
    async fn get_fee_estimate_via_msp_client() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(200)
            .with_body(r#"{"success":true,"estimates":[100,500]}"#)
            .create_async()
            .await;

        let payload = get_fee_estimate("mainnet", Some(&server.url()), vec![300], 1_000_000, None)
            .await
            .expect("fee estimate");
        assert_eq!(conservative_fee_from_payload(&payload), Some(500));
    }
}
