//! Reusable / unreturned presplit maker queries.

use crate::error::SignerResult;
use crate::offer::request::DEFAULT_OFFER_SIDE;
use rusqlite::params;

use super::super::{query_mapped, SqliteStore};
use super::{
    paginate_all, read_reusable_presplit_maker_row, state_in_placeholders,
    DURABLE_MAKER_CANCEL_METADATA_SQL, REUSABLE_PAGE_SIZE, UNRETURNED_PAGE_SIZE,
};

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

struct ReusableMakerQuery<'a> {
    market_id: &'a str,
    size_base_units: Option<i64>,
    side: Option<&'a str>,
    states: &'a [&'a str],
}

impl SqliteStore {
    /// Expired (or soft-expired) presplit makers that can be reused or reclaimed for size N.
    ///
    /// NULL / blank `offer_side` matches [`crate::offer::request::DEFAULT_OFFER_SIDE`] so
    /// ensure can `PreferExisting` makers soft-expire left for a sell-side gap.
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
        self.query_reusable_presplit_makers(market_id, Some(size_base_units), side, &["expired"])
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
        self.query_reusable_presplit_makers(market_id, None, None, &["expired"])
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
        paginate_all(UNRETURNED_PAGE_SIZE, |limit, offset| {
            self.list_unreturned_presplit_makers_page(market_filter, limit, offset)
        })
    }

    fn list_unreturned_presplit_makers_page(
        &self,
        market_filter: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        let sql = format!(
            r"
            SELECT offer_id, market_id, state, size_base_units, offer_side,
                   cancel_input_coin_id, fixed_delegated_puzzle_hash, offer_nonce, listing_expires_at
            FROM offer_state
            WHERE (?1 IS NULL OR market_id = ?1)
              {DURABLE_MAKER_CANCEL_METADATA_SQL}
            ORDER BY updated_at DESC, offer_id DESC
            LIMIT ?2 OFFSET ?3
            "
        );
        query_mapped(
            &self.conn,
            &sql,
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
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        let filter = ReusableMakerQuery {
            market_id,
            size_base_units,
            side,
            states,
        };
        paginate_all(REUSABLE_PAGE_SIZE, |limit, offset| {
            self.query_reusable_presplit_makers_page(&filter, limit, offset)
        })
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
              {DURABLE_MAKER_CANCEL_METADATA_SQL}
              AND (?2 IS NULL OR size_base_units = ?2)
              AND (
                    ?3 IS NULL
                    OR lower(COALESCE(NULLIF(TRIM(offer_side), ''), ?4)) = lower(?3)
                  )
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
            DEFAULT_OFFER_SIDE.to_string().into(),
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
