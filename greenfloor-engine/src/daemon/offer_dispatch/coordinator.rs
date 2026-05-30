use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

const DEFAULT_LEASE_SECONDS: i64 = 300;

#[derive(Debug)]
pub struct OfferReservationCoordinator {
    db_path: PathBuf,
    lease_seconds: i64,
    lock: Mutex<()>,
}

impl OfferReservationCoordinator {
    pub fn new(db_path: impl AsRef<Path>, lease_seconds: Option<i64>) -> Self {
        let lease_seconds = lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS).max(30);
        Self {
            db_path: db_path.as_ref().to_path_buf(),
            lease_seconds,
            lock: Mutex::new(()),
        }
    }

    pub fn try_acquire(
        &self,
        market_id: &str,
        wallet_id: &str,
        requested_amounts: &BTreeMap<String, i64>,
        available_amounts: &BTreeMap<String, i64>,
    ) -> SignerResult<ReservationAcquireResult> {
        let _guard = self.lock.lock().map_err(|err| {
            SignerError::Other(format!("reservation coordinator lock poisoned: {err}"))
        })?;
        let store = SqliteStore::open(&self.db_path)?;
        let reservation_id = format!(
            "res-{:x}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0),
            std::process::id()
        );
        match store.try_acquire_offer_reservation_lease(
            &reservation_id,
            market_id,
            wallet_id,
            requested_amounts,
            available_amounts,
            self.lease_seconds,
            None,
        )? {
            None => Ok(ReservationAcquireResult {
                ok: true,
                reservation_id: Some(reservation_id),
                error: None,
            }),
            Some(error) => {
                if error.contains("reservation_insufficient") {
                    return Ok(ReservationAcquireResult {
                        ok: false,
                        reservation_id: None,
                        error: Some(error),
                    });
                }
                Ok(ReservationAcquireResult {
                    ok: false,
                    reservation_id: None,
                    error: Some(error),
                })
            }
        }
    }

    pub fn release(&self, reservation_id: &str, release_status: &str) -> SignerResult<()> {
        let _guard = self.lock.lock().map_err(|err| {
            SignerError::Other(format!("reservation coordinator lock poisoned: {err}"))
        })?;
        let store = SqliteStore::open(&self.db_path)?;
        store.release_offer_reservation_lease(reservation_id, release_status)?;
        Ok(())
    }

    pub fn expire_stale(&self) -> SignerResult<u64> {
        let _guard = self.lock.lock().map_err(|err| {
            SignerError::Other(format!("reservation coordinator lock poisoned: {err}"))
        })?;
        let store = SqliteStore::open(&self.db_path)?;
        store.expire_offer_reservation_leases(None)
    }
}

#[derive(Debug, Clone)]
pub struct ReservationAcquireResult {
    pub ok: bool,
    pub reservation_id: Option<String>,
    pub error: Option<String>,
}
