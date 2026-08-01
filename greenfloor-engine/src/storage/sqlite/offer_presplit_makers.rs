//! Soft-expiry / vault-balance queries over persisted presplit maker rows.

use crate::error::SignerResult;
use rusqlite::params;

use super::{query_mapped, SqliteStore};

const REUSABLE_PAGE_SIZE: i64 = 200;
const UNRETURNED_PAGE_SIZE: i64 = 500;

/// Presplit maker row with cancel metadata used for soft-expire reuse / reclaim / balance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusablePresplitMakerRow {
    pub offer_id: String,
    pub market_id: String,
    pub state: String,
    pub size_base_units: Option<i64>,
    pub offer_side: Option<String>,
    pub cancel_input_coin_id: String,
    pub fixed_delegated_puzzle_hash: String,
    pub offer_nonce: Option<String>,
    pub listing_expires_at: Option<i64>,
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

fn state_in_placeholders(start: usize, count: usize) -> String {
    (0..count)
        .map(|idx| format!("?{}", start + idx))
        .collect::<Vec<_>>()
        .join(", ")
}

impl SqliteStore {
    /// Open offers whose soft listing expiry has elapsed (`listing_expires_at <= now_unix`).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_open_offers_past_listing_expiry(
        &self,
        market_id: &str,
        now_unix: i64,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        self.query_reusable_presplit_makers(
            market_id,
            None,
            None,
            &["open", "refresh_due"],
            Some(now_unix),
        )
    }

    /// Expired (or soft-expired) presplit makers that can be reused or reclaimed for size N.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_reusable_presplit_makers_for_size(
        &self,
        market_id: &str,
        size_base_units: i64,
        side: Option<&str>,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        self.query_reusable_presplit_makers(
            market_id,
            Some(size_base_units),
            side,
            &["expired"],
            None,
        )
    }

    /// All expired presplit makers for a market (any size).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_expired_presplit_makers(
        &self,
        market_id: &str,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        self.query_reusable_presplit_makers(market_id, None, None, &["expired"], None)
    }

    /// Known maker rows with cancel metadata for vault-controlled balance / unreturned listing.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_unreturned_presplit_makers(
        &self,
        market_id: Option<&str>,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        let market_filter = market_id.map(str::trim).filter(|id| !id.is_empty());
        let mut all = Vec::new();
        let mut offset: i64 = 0;
        loop {
            let page = self.list_unreturned_presplit_makers_page(
                market_filter,
                UNRETURNED_PAGE_SIZE,
                offset,
            )?;
            let page_len = i64::try_from(page.len()).unwrap_or(i64::MAX);
            all.extend(page);
            if page_len < UNRETURNED_PAGE_SIZE {
                break;
            }
            offset = offset.saturating_add(UNRETURNED_PAGE_SIZE);
        }
        Ok(all)
    }

    fn list_unreturned_presplit_makers_page(
        &self,
        market_filter: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        let sql = r"
            SELECT offer_id, market_id, state, size_base_units, offer_side,
                   cancel_input_coin_id, fixed_delegated_puzzle_hash, offer_nonce, listing_expires_at
            FROM offer_state
            WHERE cancel_input_coin_id IS NOT NULL
              AND TRIM(cancel_input_coin_id) != ''
              AND fixed_delegated_puzzle_hash IS NOT NULL
              AND TRIM(fixed_delegated_puzzle_hash) != ''
              AND (?1 IS NULL OR market_id = ?1)
            ORDER BY updated_at DESC, offer_id DESC
            LIMIT ?2 OFFSET ?3
        ";
        query_mapped(
            &self.conn,
            sql,
            params![market_filter, limit, offset],
            "unreturned presplit makers",
            read_reusable_presplit_maker_row,
        )
    }

    fn query_reusable_presplit_makers(
        &self,
        market_id: &str,
        size_base_units: Option<i64>,
        side: Option<&str>,
        states: &[&str],
        past_listing_expiry: Option<i64>,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        let mut all = Vec::new();
        let mut offset: i64 = 0;
        let filter = ReusableMakerQuery {
            market_id,
            size_base_units,
            side,
            states,
            past_listing_expiry,
        };
        loop {
            let page =
                self.query_reusable_presplit_makers_page(&filter, REUSABLE_PAGE_SIZE, offset)?;
            let page_len = i64::try_from(page.len()).unwrap_or(i64::MAX);
            all.extend(page);
            if page_len < REUSABLE_PAGE_SIZE {
                break;
            }
            offset = offset.saturating_add(REUSABLE_PAGE_SIZE);
        }
        Ok(all)
    }

    fn query_reusable_presplit_makers_page(
        &self,
        filter: &ReusableMakerQuery<'_>,
        limit: i64,
        offset: i64,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        if filter.states.is_empty() {
            return Ok(Vec::new());
        }
        let state_placeholders = state_in_placeholders(5, filter.states.len());
        let limit_idx = 5 + filter.states.len();
        let offset_idx = limit_idx + 1;
        let sql = format!(
            r"
            SELECT offer_id, market_id, state, size_base_units, offer_side,
                   cancel_input_coin_id, fixed_delegated_puzzle_hash, offer_nonce, listing_expires_at
            FROM offer_state
            WHERE market_id = ?1
              AND cancel_input_coin_id IS NOT NULL
              AND TRIM(cancel_input_coin_id) != ''
              AND fixed_delegated_puzzle_hash IS NOT NULL
              AND TRIM(fixed_delegated_puzzle_hash) != ''
              AND (?2 IS NULL OR size_base_units = ?2)
              AND (?3 IS NULL OR lower(COALESCE(offer_side, '')) = lower(?3))
              AND (?4 IS NULL OR (listing_expires_at IS NOT NULL AND listing_expires_at <= ?4))
              AND state IN ({state_placeholders})
            ORDER BY updated_at ASC, offer_id ASC
            LIMIT ?{limit_idx} OFFSET ?{offset_idx}
            "
        );
        let mut values: Vec<rusqlite::types::Value> = vec![
            filter.market_id.to_string().into(),
            filter
                .size_base_units
                .map_or(rusqlite::types::Value::Null, rusqlite::types::Value::from),
            filter
                .side
                .map_or(rusqlite::types::Value::Null, |s| s.to_string().into()),
            filter
                .past_listing_expiry
                .map_or(rusqlite::types::Value::Null, rusqlite::types::Value::from),
        ];
        values.extend(
            filter
                .states
                .iter()
                .map(|state| (*state).to_string().into()),
        );
        values.push(limit.into());
        values.push(offset.into());
        query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(values),
            "reusable presplit makers",
            read_reusable_presplit_maker_row,
        )
    }
}

struct ReusableMakerQuery<'a> {
    market_id: &'a str,
    size_base_units: Option<i64>,
    side: Option<&'a str>,
    states: &'a [&'a str],
    past_listing_expiry: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::super::offer_cancel::{OfferCancelWrite, OfferListingWrite};
    use super::{SqliteStore, REUSABLE_PAGE_SIZE};
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use tempfile::tempdir;

    fn insert_expired_maker(store: &SqliteStore, offer_id: &str, coin_suffix: u8) {
        let coin = format!("{coin_suffix:02x}").repeat(32);
        let fields =
            OfferCancelFields::from_presplit_build(coin.clone(), "bb".repeat(32), "cc".repeat(32));
        store
            .upsert_offer_state_with_metadata_at(
                offer_id,
                "m1",
                "expired",
                Some(6),
                "2026-01-01T00:00:00Z",
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    listing: OfferListingWrite {
                        publish_venue: Some("dexie"),
                        listing_expires_at: Some(1_700_000_000),
                        size_base_units: Some(10),
                        offer_nonce: Some(&"dd".repeat(32)),
                        offer_side: Some("sell"),
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
    }

    #[test]
    fn soft_listing_expiry_and_expired_maker_queries() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let fields = OfferCancelFields::from_presplit_build(
            "aa".repeat(32),
            "bb".repeat(32),
            "cc".repeat(32),
        );
        let nonce = "dd".repeat(32);
        store
            .upsert_offer_state_with_metadata_at(
                "offer-soft",
                "m1",
                "open",
                None,
                "2026-01-01T00:00:00Z",
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::PresplitExisting),
                    listing: OfferListingWrite {
                        publish_venue: Some("dexie"),
                        listing_expires_at: Some(1_700_000_000),
                        size_base_units: Some(10),
                        offer_nonce: Some(nonce.as_str()),
                        offer_side: Some("sell"),
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
        let past = store
            .list_open_offers_past_listing_expiry("m1", 1_700_000_001)
            .expect("past");
        assert_eq!(past.len(), 1);
        assert_eq!(past[0].offer_id, "offer-soft");
        store
            .upsert_offer_state("offer-soft", "m1", "expired", Some(6))
            .expect("expire");
        let expired = store
            .list_reusable_presplit_makers_for_size("m1", 10, Some("sell"))
            .expect("expired makers");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].offer_nonce.as_deref(), Some(nonce.as_str()));
        let unreturned = store
            .list_unreturned_presplit_makers(Some("m1"))
            .expect("unreturned");
        assert_eq!(unreturned.len(), 1);
        assert_eq!(unreturned[0].cancel_input_coin_id, "aa".repeat(32));
        assert_eq!(unreturned[0].fixed_delegated_puzzle_hash, "bb".repeat(32));
    }

    #[test]
    fn expired_maker_queries_paginate_past_page_size() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let total = usize::try_from(REUSABLE_PAGE_SIZE).expect("page") + 3;
        for idx in 0..total {
            insert_expired_maker(
                &store,
                &format!("offer-{idx:04}"),
                u8::try_from(idx % 250).unwrap_or(0),
            );
        }
        let expired = store.list_expired_presplit_makers("m1").expect("list");
        assert_eq!(expired.len(), total);
    }
}
