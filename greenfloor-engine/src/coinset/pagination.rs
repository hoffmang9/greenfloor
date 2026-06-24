//! Coinset cursor pagination for coin-record query endpoints.
//!
//! Coinset may truncate large responses (`truncated: true`) and return `next_cursor`
//! for follow-up requests. The public docs omit these fields; see `docs/COINSET_DOCS_AND_API.md`.

use std::future::Future;

use chia_protocol::Bytes32;
use chia_sdk_coinset::{ChiaRpcClient, CoinRecord, CoinsetClient, GetCoinRecordsResponse};
use serde_json::Value;

use super::parse::{
    coin_records_from_payload, coin_records_page_from_response, ensure_complete_page,
    pagination_from_payload,
};
use super::retry::with_coinset_client_retries;
use crate::error::{SignerError, SignerResult};

const MAX_COINSET_RECORD_PAGES: usize = 10_000;

/// Fetch all coin records for a puzzle hash, following Coinset cursor pages when present.
///
/// # Errors
///
/// Returns an error if any page fails or a truncated page lacks `next_cursor`.
pub(crate) async fn coin_records_by_puzzle_hash(
    client: &CoinsetClient,
    puzzle_hash: Bytes32,
    start_height: Option<u32>,
    end_height: Option<u32>,
    include_spent_coins: Option<bool>,
) -> SignerResult<Vec<CoinRecord>> {
    fetch_paginated_typed_records(|cursor| {
        let client = client.clone();
        async move {
            with_coinset_client_retries(|| {
                let client = client.clone();
                let cursor = cursor.clone();
                async move {
                    client
                        .get_coin_records_by_puzzle_hash(
                            puzzle_hash,
                            start_height,
                            end_height,
                            include_spent_coins,
                            cursor,
                        )
                        .await
                }
            })
            .await
        }
    })
    .await
}

async fn fetch_paginated_typed_records<F, Fut>(mut fetch_page: F) -> SignerResult<Vec<CoinRecord>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = SignerResult<GetCoinRecordsResponse>>,
{
    let mut all = Vec::new();
    let mut cursor = None;
    for _page_idx in 0..MAX_COINSET_RECORD_PAGES {
        let response = fetch_page(cursor.take()).await?;
        let (page, pagination) = coin_records_page_from_response(response)?;
        all.extend(page);
        if pagination.truncated {
            ensure_complete_page(&pagination)?;
            cursor = pagination.next_cursor;
            continue;
        }
        return Ok(all);
    }
    Err(SignerError::Coinset(format!(
        "coinset pagination exceeded {MAX_COINSET_RECORD_PAGES} pages"
    )))
}

/// Fetch all coin records from a JSON Coinset endpoint, following cursor pages when present.
///
/// # Errors
///
/// Returns an error if any page fails or a truncated page lacks `next_cursor`.
pub(crate) async fn coin_records_from_json_endpoint<F, Fut>(
    mut fetch_payload: F,
) -> SignerResult<Vec<Value>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = SignerResult<Value>>,
{
    let mut all = Vec::new();
    let mut cursor = None;
    for _page_idx in 0..MAX_COINSET_RECORD_PAGES {
        let payload = fetch_payload(cursor.take()).await?;
        let records = coin_records_from_payload(&payload)?;
        let pagination = pagination_from_payload(&payload);
        ensure_complete_page(&pagination)?;
        all.extend(records);
        if pagination.truncated {
            cursor = pagination.next_cursor;
            continue;
        }
        return Ok(all);
    }
    Err(SignerError::Coinset(format!(
        "coinset pagination exceeded {MAX_COINSET_RECORD_PAGES} pages"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_sdk_coinset::CoinRecord;
    use serde_json::json;

    fn sample_record(amount: u64) -> CoinRecord {
        use chia_protocol::{Bytes32, Coin};

        CoinRecord {
            coin: Coin::new(Bytes32::new([0x11; 32]), Bytes32::new([0x22; 32]), amount),
            confirmed_block_index: 1,
            spent_block_index: 0,
            spent: false,
            coinbase: false,
            timestamp: 1,
        }
    }

    #[tokio::test]
    async fn fetch_paginated_typed_records_follows_cursor() {
        let pages = fetch_paginated_typed_records(|cursor| async move {
            match cursor {
                None => Ok(GetCoinRecordsResponse {
                    coin_records: Some(vec![sample_record(1)]),
                    error: None,
                    success: true,
                    truncated: Some(true),
                    next_cursor: Some("page-2".to_string()),
                }),
                Some(cursor) if cursor == "page-2" => Ok(GetCoinRecordsResponse {
                    coin_records: Some(vec![sample_record(2)]),
                    error: None,
                    success: true,
                    truncated: None,
                    next_cursor: None,
                }),
                Some(other) => Err(SignerError::Coinset(format!("unexpected cursor {other}"))),
            }
        })
        .await
        .expect("paginated fetch");

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].coin.amount, 1);
        assert_eq!(pages[1].coin.amount, 2);
    }

    #[tokio::test]
    async fn fetch_paginated_typed_records_errors_when_truncated_without_cursor() {
        let err = fetch_paginated_typed_records(|_cursor| async {
            Ok(GetCoinRecordsResponse {
                coin_records: Some(vec![sample_record(1)]),
                error: None,
                success: true,
                truncated: Some(true),
                next_cursor: None,
            })
        })
        .await
        .expect_err("missing cursor");
        assert!(err.to_string().contains("truncated without next_cursor"));
    }

    #[tokio::test]
    async fn coin_records_from_json_endpoint_follows_cursor() {
        let records = coin_records_from_json_endpoint(|cursor| async move {
            match cursor {
                None => Ok(json!({
                    "success": true,
                    "truncated": true,
                    "next_cursor": "page-2",
                    "coin_records": [{"coin": {"amount": 1}}]
                })),
                Some(cursor) if cursor == "page-2" => Ok(json!({
                    "success": true,
                    "coin_records": [{"coin": {"amount": 2}}]
                })),
                Some(other) => Err(SignerError::Coinset(format!("unexpected cursor {other}"))),
            }
        })
        .await
        .expect("json pagination");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["coin"]["amount"], 1);
        assert_eq!(records[1]["coin"]["amount"], 2);
    }
}
