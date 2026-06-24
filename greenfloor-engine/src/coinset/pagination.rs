//! Coinset cursor pagination for coin-record query endpoints.
//!
//! Coinset may truncate large responses (`truncated: true`) and return `next_cursor`
//! for follow-up requests. The public docs omit these fields; see `docs/COINSET_DOCS_AND_API.md`.

use std::future::Future;

use chia_protocol::Bytes32;
use chia_sdk_coinset::{ChiaRpcClient, CoinRecord, CoinsetClient, GetCoinRecordsResponse};

use super::parse::{
    coin_records_page_from_response, ensure_complete_page, CoinsetRecordsPagination,
};
use super::retry::with_coinset_client_retries;
use crate::error::{SignerError, SignerResult};
use crate::operator_log::LogContext;

const MAX_COINSET_RECORD_PAGES: usize = 10_000;

async fn fetch_all_coinset_pages<T, F, Fut>(
    endpoint: &str,
    mut fetch_page: F,
) -> SignerResult<Vec<T>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = SignerResult<(Vec<T>, CoinsetRecordsPagination)>>,
{
    let mut all = Vec::new();
    let mut cursor = None;
    for page_index in 0..MAX_COINSET_RECORD_PAGES {
        let (page, pagination) = fetch_page(cursor.take()).await?;
        let page_record_count = page.len();
        all.extend(page);
        crate::trace_event!(
            DEBUG,
            LogContext::COINSET,
            "coinset_page_fetched",
            {
                endpoint = endpoint,
                page_index = page_index,
                page_record_count = page_record_count,
                total_record_count = all.len(),
                truncated = pagination.truncated,
            };
            "fetched coinset coin-record page"
        );
        if pagination.truncated {
            ensure_complete_page(&pagination)?;
            cursor = pagination.next_cursor;
            continue;
        }
        return Ok(all);
    }
    Err(SignerError::Coinset(format!(
        "coinset pagination exceeded {MAX_COINSET_RECORD_PAGES} pages for {endpoint}"
    )))
}

async fn typed_coin_records_page<F, Fut>(
    fetch: F,
) -> SignerResult<(Vec<CoinRecord>, CoinsetRecordsPagination)>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<GetCoinRecordsResponse, reqwest::Error>>,
{
    let response = with_coinset_client_retries(fetch).await?;
    coin_records_page_from_response(response)
}

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
    let client = client.clone();
    fetch_all_coinset_pages("get_coin_records_by_puzzle_hash", move |cursor| {
        let client = client.clone();
        async move {
            typed_coin_records_page(|| {
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

/// Fetch all coin records for parent ids, following Coinset cursor pages when present.
///
/// # Errors
///
/// Returns an error if any page fails or a truncated page lacks `next_cursor`.
pub(crate) async fn coin_records_by_parent_ids(
    client: &CoinsetClient,
    parent_ids: Vec<Bytes32>,
    start_height: Option<u32>,
    end_height: Option<u32>,
    include_spent_coins: Option<bool>,
) -> SignerResult<Vec<CoinRecord>> {
    let client = client.clone();
    fetch_all_coinset_pages("get_coin_records_by_parent_ids", move |cursor| {
        let client = client.clone();
        let parent_ids = parent_ids.clone();
        async move {
            typed_coin_records_page(|| {
                let client = client.clone();
                let cursor = cursor.clone();
                let parent_ids = parent_ids.clone();
                async move {
                    client
                        .get_coin_records_by_parent_ids(
                            parent_ids,
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

/// Fetch all coin records from a JSON Coinset endpoint, following cursor pages when present.
///
/// # Errors
///
/// Returns an error if any page fails or a truncated page lacks `next_cursor`.
pub(crate) async fn coin_records_from_json_endpoint<T, F, Fut>(
    endpoint: &str,
    fetch_page: F,
) -> SignerResult<Vec<T>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = SignerResult<(Vec<T>, CoinsetRecordsPagination)>>,
{
    fetch_all_coinset_pages(endpoint, fetch_page).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_sdk_coinset::{CoinRecord, GetCoinRecordsResponse};
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
    async fn fetch_all_coinset_pages_follows_cursor_for_typed_responses() {
        let pages =
            fetch_all_coinset_pages("get_coin_records_by_puzzle_hash", |cursor| async move {
                match cursor {
                    None => coin_records_page_from_response(GetCoinRecordsResponse {
                        coin_records: Some(vec![sample_record(1)]),
                        error: None,
                        success: true,
                        truncated: Some(true),
                        next_cursor: Some("page-2".to_string()),
                    }),
                    Some(cursor) if cursor == "page-2" => {
                        coin_records_page_from_response(GetCoinRecordsResponse {
                            coin_records: Some(vec![sample_record(2)]),
                            error: None,
                            success: true,
                            truncated: None,
                            next_cursor: None,
                        })
                    }
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
    async fn fetch_all_coinset_pages_errors_when_truncated_without_cursor() {
        let err = fetch_all_coinset_pages("get_coin_records_by_puzzle_hash", |_cursor| async {
            coin_records_page_from_response(GetCoinRecordsResponse {
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
        let records = coin_records_from_json_endpoint(
            "get_coin_records_by_puzzle_hashes",
            |cursor| async move {
                match cursor {
                    None => Ok((
                        vec![json!({"coin": {"amount": 1}})],
                        CoinsetRecordsPagination {
                            truncated: true,
                            next_cursor: Some("page-2".to_string()),
                        },
                    )),
                    Some(cursor) if cursor == "page-2" => Ok((
                        vec![json!({"coin": {"amount": 2}})],
                        CoinsetRecordsPagination {
                            truncated: false,
                            next_cursor: None,
                        },
                    )),
                    Some(other) => Err(SignerError::Coinset(format!("unexpected cursor {other}"))),
                }
            },
        )
        .await
        .expect("json pagination");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["coin"]["amount"], 1);
        assert_eq!(records[1]["coin"]["amount"], 2);
    }
}
