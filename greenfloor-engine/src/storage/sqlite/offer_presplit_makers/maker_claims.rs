//! Fenced `maker_claimed` lease CAS for ensure / soft-expire.

use crate::cycle::reconcile::{STATE_CANCELLED, STATE_MAKER_CLAIMED};
use crate::error::SignerResult;
use crate::offer::dexie_payload::{DEXIE_STATUS_CANCELLED, DEXIE_STATUS_EXPIRED};
use rusqlite::params;

use super::super::{utcnow_iso, SqliteStore};

/// Soft-expire / ensure reclaim stuck `maker_claimed` rows older than this.
///
/// Live workers holding a claim must renew `updated_at` more often than this
/// ([`MAKER_CLAIM_RENEW_INTERVAL_SECONDS`]) so stale recovery cannot steal in-flight I/O.
pub const MAKER_CLAIM_STALE_SECONDS: i64 = 600;

/// Heartbeat interval while holding a claim during external PreferExisting/reclaim I/O.
pub const MAKER_CLAIM_RENEW_INTERVAL_SECONDS: u64 = 60;

impl SqliteStore {
    /// CAS-claim an expired maker (`expired` → `maker_claimed`) with a fencing token.
    ///
    /// Does not mark Dexie-cancelled; call [`Self::finalize_maker_claim`] after successful
    /// `PreferExisting`/reclaim, or [`Self::restore_maker_claim`] when the coin stays reusable.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub fn try_claim_expired_maker(
        &self,
        offer_id: &str,
        market_id: &str,
        claim_token: &str,
    ) -> SignerResult<bool> {
        let changed = self
            .conn
            .execute(
                r"
                UPDATE offer_state
                SET state = ?1, maker_claim_token = ?2, updated_at = ?3
                WHERE offer_id = ?4 AND market_id = ?5 AND state = 'expired'
                ",
                params![
                    STATE_MAKER_CLAIMED,
                    claim_token,
                    utcnow_iso(),
                    offer_id,
                    market_id
                ],
            )
            .map_err(|err| {
                crate::error::SignerError::Other(format!("try claim expired maker: {err}"))
            })?;
        Ok(changed == 1)
    }

    /// Undo [`Self::try_claim_expired_maker`] when I/O left the maker coin reusable.
    ///
    /// Returns `true` when this token still owned the claim.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub fn restore_maker_claim(
        &self,
        offer_id: &str,
        market_id: &str,
        claim_token: &str,
    ) -> SignerResult<bool> {
        let changed = self
            .conn
            .execute(
                r"
                UPDATE offer_state
                SET state = 'expired', last_seen_status = ?1, updated_at = ?2,
                    maker_claim_token = NULL
                WHERE offer_id = ?3 AND market_id = ?4 AND state = ?5
                  AND maker_claim_token = ?6
                ",
                params![
                    DEXIE_STATUS_EXPIRED,
                    utcnow_iso(),
                    offer_id,
                    market_id,
                    STATE_MAKER_CLAIMED,
                    claim_token
                ],
            )
            .map_err(|err| {
                crate::error::SignerError::Other(format!("restore maker claim: {err}"))
            })?;
        Ok(changed == 1)
    }

    /// Extend an in-flight claim lease by refreshing `updated_at` (fencing token CAS).
    ///
    /// Returns `true` when this token still owned the claim.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub fn renew_maker_claim(
        &self,
        offer_id: &str,
        market_id: &str,
        claim_token: &str,
    ) -> SignerResult<bool> {
        let changed = self
            .conn
            .execute(
                r"
                UPDATE offer_state
                SET updated_at = ?1
                WHERE offer_id = ?2 AND market_id = ?3 AND state = ?4
                  AND maker_claim_token = ?5
                ",
                params![
                    utcnow_iso(),
                    offer_id,
                    market_id,
                    STATE_MAKER_CLAIMED,
                    claim_token
                ],
            )
            .map_err(|err| crate::error::SignerError::Other(format!("renew maker claim: {err}")))?;
        Ok(changed == 1)
    }

    /// Commit a successful claim (`maker_claimed` → `cancelled` + Dexie cancelled).
    ///
    /// Returns `true` when this token still owned the claim.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub fn finalize_maker_claim(
        &self,
        offer_id: &str,
        market_id: &str,
        claim_token: &str,
    ) -> SignerResult<bool> {
        let changed = self
            .conn
            .execute(
                r"
                UPDATE offer_state
                SET state = ?1, last_seen_status = ?2, updated_at = ?3,
                    maker_claim_token = NULL
                WHERE offer_id = ?4 AND market_id = ?5 AND state = ?6
                  AND maker_claim_token = ?7
                ",
                params![
                    STATE_CANCELLED,
                    DEXIE_STATUS_CANCELLED,
                    utcnow_iso(),
                    offer_id,
                    market_id,
                    STATE_MAKER_CLAIMED,
                    claim_token
                ],
            )
            .map_err(|err| {
                crate::error::SignerError::Other(format!("finalize maker claim: {err}"))
            })?;
        Ok(changed == 1)
    }

    /// Restore crashed/stale `maker_claimed` rows older than [`MAKER_CLAIM_STALE_SECONDS`].
    ///
    /// Clears `maker_claim_token` so a late worker cannot finalize/restore after reclaim.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub fn restore_stale_maker_claims(&self, now_unix: i64) -> SignerResult<u64> {
        let cutoff_unix = now_unix.saturating_sub(MAKER_CLAIM_STALE_SECONDS);
        let cutoff = chrono::DateTime::from_timestamp(cutoff_unix, 0)
            .map_or_else(|| "1970-01-01T00:00:00Z".to_string(), |dt| dt.to_rfc3339());
        let changed = self
            .conn
            .execute(
                r"
                UPDATE offer_state
                SET state = 'expired', last_seen_status = ?1, updated_at = ?2,
                    maker_claim_token = NULL
                WHERE state = ?3 AND updated_at < ?4
                ",
                params![
                    DEXIE_STATUS_EXPIRED,
                    utcnow_iso(),
                    STATE_MAKER_CLAIMED,
                    cutoff
                ],
            )
            .map_err(|err| {
                crate::error::SignerError::Other(format!("restore stale maker claims: {err}"))
            })?;
        Ok(u64::try_from(changed).unwrap_or(0))
    }
}
