use chia_sdk_coinset::{CoinRecord, GetCoinRecordsResponse};

use super::cursor::{pagination_from_response, CoinsetRecordsPagination};
use crate::coinset::rpc_result::ensure_coinset_success;
use crate::error::SignerResult;

pub(crate) fn coin_records_page_from_response(
    response: GetCoinRecordsResponse,
) -> SignerResult<(Vec<CoinRecord>, CoinsetRecordsPagination)> {
    ensure_coinset_success(
        response.success,
        response.error.as_deref(),
        "coinset request failed",
    )?;
    let pagination = pagination_from_response(&response);
    Ok((response.coin_records.unwrap_or_default(), pagination))
}
