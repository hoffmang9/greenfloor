//! Soft-expiry / vault-balance queries over persisted presplit maker rows.

mod listing_expiry;
mod maker_claims;
mod reusable;

#[cfg(test)]
mod tests;

use crate::error::SignerResult;

pub use listing_expiry::OfferListingFields;
pub use maker_claims::{MAKER_CLAIM_RENEW_INTERVAL_SECONDS, MAKER_CLAIM_STALE_SECONDS};
pub use reusable::ReusablePresplitMakerRow;

pub(crate) const REUSABLE_PAGE_SIZE: i64 = 200;
const UNRETURNED_PAGE_SIZE: i64 = 500;

fn paginate_all<T, F>(page_size: i64, mut fetch_page: F) -> SignerResult<Vec<T>>
where
    F: FnMut(i64, i64) -> SignerResult<Vec<T>>,
{
    let mut all = Vec::new();
    let mut offset: i64 = 0;
    loop {
        let page = fetch_page(page_size, offset)?;
        let page_len = i64::try_from(page.len()).unwrap_or(i64::MAX);
        all.extend(page);
        if page_len < page_size {
            break;
        }
        offset = offset.saturating_add(page_size);
    }
    Ok(all)
}

fn state_in_placeholders(start: usize, count: usize) -> String {
    (0..count)
        .map(|idx| format!("?{}", start + idx))
        .collect::<Vec<_>>()
        .join(", ")
}

fn read_reusable_presplit_maker_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReusablePresplitMakerRow> {
    Ok(ReusablePresplitMakerRow {
        offer_id: row.get(0)?,
        market_id: row.get(1)?,
        state: row.get(2)?,
        size_base_units: row.get(3)?,
        offer_side: row
            .get::<_, Option<String>>(4)?
            .filter(|value| !value.trim().is_empty()),
        cancel_input_coin_id: row.get(5)?,
        fixed_delegated_puzzle_hash: row.get(6)?,
        offer_nonce: row
            .get::<_, Option<String>>(7)?
            .filter(|value| !value.trim().is_empty()),
        listing_expires_at: row.get(8)?,
    })
}
