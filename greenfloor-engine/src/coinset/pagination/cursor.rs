use chia_sdk_coinset::GetCoinRecordsResponse;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoinsetRecordsPagination {
    pub truncated: bool,
    pub next_cursor: Option<String>,
}

fn pagination_fields(truncated: bool, next_cursor: Option<String>) -> CoinsetRecordsPagination {
    CoinsetRecordsPagination {
        truncated,
        next_cursor,
    }
}

pub(crate) fn pagination_from_response(
    response: &GetCoinRecordsResponse,
) -> CoinsetRecordsPagination {
    pagination_fields(
        response.truncated.unwrap_or(false),
        response.next_cursor.clone(),
    )
}

pub(crate) fn pagination_from_payload(payload: &Value) -> CoinsetRecordsPagination {
    pagination_fields(
        payload
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        payload
            .get("next_cursor")
            .and_then(Value::as_str)
            .map(str::to_string),
    )
}

/// Reject truncated Coinset pages that omit the resume cursor.
///
/// # Errors
///
/// Returns an error when `truncated` is true and `next_cursor` is missing.
pub(crate) fn ensure_complete_page(pagination: &CoinsetRecordsPagination) -> SignerResult<()> {
    if pagination.truncated && pagination.next_cursor.is_none() {
        return Err(SignerError::Coinset(
            "coinset response truncated without next_cursor".to_string(),
        ));
    }
    Ok(())
}
