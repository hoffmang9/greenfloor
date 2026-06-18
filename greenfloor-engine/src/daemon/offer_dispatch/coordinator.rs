use std::sync::Mutex;

use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

const DEFAULT_LEASE_SECONDS: i64 = 300;

pub struct OfferReservationCoordinator {
    store: Mutex<SqliteStore>,
    lease_seconds: i64,
}

impl OfferReservationCoordinator {
    pub fn new(
        db_path: impl AsRef<std::path::Path>,
        lease_seconds: Option<i64>,
    ) -> SignerResult<Self> {
        let lease_seconds = lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS).max(30);
        Ok(Self {
            store: Mutex::new(SqliteStore::open(db_path.as_ref())?),
            lease_seconds,
        })
    }

    pub fn try_acquire(
        &self,
        market_id: &str,
        wallet_id: &str,
        requested_amounts: &std::collections::BTreeMap<String, i64>,
        available_amounts: &std::collections::BTreeMap<String, i64>,
    ) -> SignerResult<ReservationAcquireResult> {
        let store = self.store.lock().map_err(|err| {
            SignerError::Other(format!("reservation coordinator lock poisoned: {err}"))
        })?;
        let reservation_id = format!(
            "res-{:x}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0),
            std::process::id()
        );
        match store.try_acquire_offer_reservation_lease(
            crate::storage::OfferReservationLeaseRequest {
                reservation_id: &reservation_id,
                market_id,
                wallet_id,
                requested_amounts,
                available_amounts,
                lease_seconds: self.lease_seconds,
                now: None,
            },
        )? {
            None => Ok(ReservationAcquireResult {
                ok: true,
                reservation_id: Some(reservation_id),
                error: None,
            }),
            Some(error) => Ok(ReservationAcquireResult {
                ok: false,
                reservation_id: None,
                error: Some(error),
            }),
        }
    }

    pub fn release(&self, reservation_id: &str, release_status: &str) -> SignerResult<()> {
        let store = self.store.lock().map_err(|err| {
            SignerError::Other(format!("reservation coordinator lock poisoned: {err}"))
        })?;
        store.release_offer_reservation_lease(reservation_id, release_status)?;
        Ok(())
    }

    pub fn expire_stale(&self) -> SignerResult<u64> {
        let store = self.store.lock().map_err(|err| {
            SignerError::Other(format!("reservation coordinator lock poisoned: {err}"))
        })?;
        store.expire_offer_reservation_leases(None)
    }
}

#[derive(Debug, Clone)]
pub struct ReservationAcquireResult {
    pub ok: bool,
    pub reservation_id: Option<String>,
    pub error: Option<String>,
}
