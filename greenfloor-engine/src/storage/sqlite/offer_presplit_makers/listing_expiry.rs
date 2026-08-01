//! Soft listing expiry mark path and listing size/side fields.

use std::collections::HashMap;

use crate::error::SignerResult;
use crate::offer::dexie_payload::DEXIE_STATUS_EXPIRED;
use rusqlite::params;

use super::super::{query_mapped, utcnow_iso, SqliteStore};
use super::{
    paginate_all, read_reusable_presplit_maker_row, state_in_placeholders,
    ReusablePresplitMakerRow, REUSABLE_PAGE_SIZE,
};

/// Persisted listing size/side on `offer_state` (canonical active-count source when set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferListingFields {
    pub size_base_units: Option<i64>,
    pub offer_side: Option<String>,
}

/// States soft-expire may mark past `listing_expires_at` (CAS source allowlist).
pub const SOFT_EXPIRE_MARK_STATES: &[&str] = &["open", "refresh_due", "mempool_observed"];

impl SqliteStore {
    /// Active listings past soft listing expiry for the soft-expire mark path.
    ///
    /// Includes `open`, `refresh_due`, and `mempool_observed` (takes/cancels in flight
    /// must still soft-expire when Dexie status 6 will not fire). NULL `listing_expires_at`
    /// counts as already elapsed (legacy). This NULL policy is specific to soft-expire
    /// marking — reusable-maker queries do not overload a shared expiry filter.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn list_open_offers_past_listing_expiry(
        &self,
        market_id: &str,
        now_unix: i64,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        paginate_all(REUSABLE_PAGE_SIZE, |limit, offset| {
            self.list_open_offers_past_listing_expiry_page(market_id, now_unix, limit, offset)
        })
    }

    fn list_open_offers_past_listing_expiry_page(
        &self,
        market_id: &str,
        now_unix: i64,
        limit: i64,
        offset: i64,
    ) -> SignerResult<Vec<ReusablePresplitMakerRow>> {
        let state_placeholders = state_in_placeholders(3, SOFT_EXPIRE_MARK_STATES.len());
        let limit_idx = 3 + SOFT_EXPIRE_MARK_STATES.len();
        let offset_idx = limit_idx + 1;
        let sql = format!(
            r"
            SELECT offer_id, market_id, state, size_base_units, offer_side,
                   cancel_input_coin_id, fixed_delegated_puzzle_hash, offer_nonce, listing_expires_at
            FROM offer_state
            WHERE market_id = ?1
              AND state IN ({state_placeholders})
              AND (listing_expires_at IS NULL OR listing_expires_at <= ?2)
            ORDER BY updated_at ASC, offer_id ASC
            LIMIT ?{limit_idx} OFFSET ?{offset_idx}
            "
        );
        let mut values: Vec<rusqlite::types::Value> =
            vec![market_id.to_string().into(), now_unix.into()];
        values.extend(
            SOFT_EXPIRE_MARK_STATES
                .iter()
                .map(|state| (*state).to_string().into()),
        );
        values.push(limit.into());
        values.push(offset.into());
        query_mapped(
            &self.conn,
            &sql,
            rusqlite::params_from_iter(values),
            "open offers past listing expiry",
            read_reusable_presplit_maker_row,
        )
    }

    /// CAS-mark one listing soft-expired when still in [`SOFT_EXPIRE_MARK_STATES`].
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub fn try_mark_listing_soft_expired(
        &self,
        offer_id: &str,
        market_id: &str,
    ) -> SignerResult<bool> {
        let state_placeholders = state_in_placeholders(5, SOFT_EXPIRE_MARK_STATES.len());
        let sql = format!(
            r"
            UPDATE offer_state
            SET state = 'expired', last_seen_status = ?1, updated_at = ?2
            WHERE offer_id = ?3 AND market_id = ?4
              AND state IN ({state_placeholders})
            "
        );
        let mut values: Vec<rusqlite::types::Value> = vec![
            DEXIE_STATUS_EXPIRED.into(),
            utcnow_iso().into(),
            offer_id.to_string().into(),
            market_id.to_string().into(),
        ];
        values.extend(
            SOFT_EXPIRE_MARK_STATES
                .iter()
                .map(|state| (*state).to_string().into()),
        );
        let changed = self
            .conn
            .execute(&sql, rusqlite::params_from_iter(values))
            .map_err(|err| {
                crate::error::SignerError::Other(format!("try mark listing soft expired: {err}"))
            })?;
        Ok(changed == 1)
    }

    /// Listing size/side for offers in a market (rows with at least one field set).
    ///
    /// Preferred over audit metadata when counting active ladder slots.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn offer_listing_fields_by_offer_id(
        &self,
        market_id: &str,
    ) -> SignerResult<HashMap<String, OfferListingFields>> {
        let sql = r"
            SELECT offer_id, size_base_units, offer_side
            FROM offer_state
            WHERE market_id = ?1
        ";
        let mut stmt = self.conn.prepare(sql).map_err(|err| {
            crate::error::SignerError::Other(format!("listing fields prepare: {err}"))
        })?;
        let rows = stmt
            .query_map(params![market_id], |row| {
                let offer_id: String = row.get(0)?;
                let size_base_units: Option<i64> = row.get(1)?;
                let offer_side: Option<String> = row
                    .get::<_, Option<String>>(2)?
                    .filter(|value| !value.trim().is_empty());
                Ok((
                    offer_id,
                    OfferListingFields {
                        size_base_units: size_base_units.filter(|units| *units > 0),
                        offer_side,
                    },
                ))
            })
            .map_err(|err| {
                crate::error::SignerError::Other(format!("listing fields query: {err}"))
            })?;
        let mut out = HashMap::new();
        for row in rows {
            let (offer_id, fields) = row.map_err(|err| {
                crate::error::SignerError::Other(format!("listing fields row: {err}"))
            })?;
            if fields.size_base_units.is_some() || fields.offer_side.is_some() {
                out.insert(offer_id, fields);
            }
        }
        Ok(out)
    }
}
