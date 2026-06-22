mod sql;
mod types;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::error::SignerResult;

use super::{sqlite_rows_changed, utcnow_iso, SqliteStore};

pub use types::{
    OfferReservationAcquireOutcome, OfferReservationLeaseRequest, OfferReservationLeaseRow,
    OfferReservationRejectReason,
};

use sql::{
    expire_stale_leases, insert_active_leases, list_leases, prune_inactive_leases,
    query_active_reserved_by_asset, release_lease,
};
use types::first_insufficient_asset;

impl SqliteStore {
    /// Try acquire offer reservation lease.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn try_acquire_offer_reservation_lease(
        &self,
        request: &OfferReservationLeaseRequest<'_>,
    ) -> SignerResult<OfferReservationAcquireOutcome> {
        match request.try_normalize()? {
            Ok(normalized) => self.immediate_transaction("reservation", |store| {
                expire_stale_leases(&store.conn, &normalized.now_iso)?;
                let reserved_by_asset = query_active_reserved_by_asset(
                    &store.conn,
                    normalized.wallet_id,
                    &normalized.now_iso,
                )?;
                if let Some(reason) = first_insufficient_asset(
                    &normalized.normalized_requests,
                    &normalized.normalized_available,
                    &reserved_by_asset,
                ) {
                    return Ok(OfferReservationAcquireOutcome::Rejected(reason));
                }
                insert_active_leases(
                    &store.conn,
                    normalized.reservation_id,
                    normalized.market_id,
                    normalized.wallet_id,
                    &normalized.normalized_requests,
                    &normalized.now_iso,
                    &normalized.expires_at_iso,
                )?;
                Ok(OfferReservationAcquireOutcome::Acquired)
            }),
            Err(reason) => Ok(OfferReservationAcquireOutcome::Rejected(reason)),
        }
    }

    /// Release offer reservation lease.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn release_offer_reservation_lease(
        &self,
        reservation_id: &str,
        release_status: &str,
    ) -> SignerResult<u64> {
        let released_at = Utc::now().to_rfc3339();
        sqlite_rows_changed(release_lease(
            &self.conn,
            reservation_id,
            release_status,
            &released_at,
        )?)
    }

    /// Expire offer reservation leases.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn expire_offer_reservation_leases(&self, now: Option<DateTime<Utc>>) -> SignerResult<u64> {
        let now_iso = now.unwrap_or_else(Utc::now).to_rfc3339();
        sqlite_rows_changed(expire_stale_leases(&self.conn, &now_iso)?)
    }

    /// List offer reservation leases.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn list_offer_reservation_leases(
        &self,
        reservation_id: Option<&str>,
    ) -> SignerResult<Vec<OfferReservationLeaseRow>> {
        list_leases(&self.conn, reservation_id)
    }

    /// Active reserved amounts for `wallet_id`, using the current wall clock.
    ///
    /// Unlike [`Self::try_acquire_offer_reservation_lease`], this read path does not
    /// accept an injectable `now`; callers needing deterministic time should expire
    /// stale leases first or query leases directly.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn get_offer_reserved_amounts_by_asset(
        &self,
        wallet_id: &str,
    ) -> SignerResult<BTreeMap<String, i64>> {
        query_active_reserved_by_asset(&self.conn, wallet_id, &utcnow_iso())
    }

    /// Prune offer reservation leases.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn prune_offer_reservation_leases(&self, older_than: DateTime<Utc>) -> SignerResult<u64> {
        let cutoff_iso = older_than.to_rfc3339();
        sqlite_rows_changed(prune_inactive_leases(&self.conn, &cutoff_iso)?)
    }
}
