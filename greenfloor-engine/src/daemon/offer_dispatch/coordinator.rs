use crate::error::SignerResult;
use crate::storage::{
    CycleWriteStore, OfferReservationAcquireOutcome, OfferReservationRejectReason,
};

const DEFAULT_LEASE_SECONDS: i64 = 300;

pub struct OfferReservationCoordinator {
    store: CycleWriteStore,
    lease_seconds: i64,
}

impl OfferReservationCoordinator {
    pub fn new(store: CycleWriteStore, lease_seconds: Option<i64>) -> Self {
        let lease_seconds = lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS).max(30);
        Self {
            store,
            lease_seconds,
        }
    }

    pub fn try_acquire(
        &self,
        market_id: &str,
        wallet_id: &str,
        requested_amounts: &std::collections::BTreeMap<String, i64>,
        available_amounts: &std::collections::BTreeMap<String, i64>,
    ) -> SignerResult<ReservationAcquireResult> {
        let store = self.store.lock()?;
        let reservation_id = format!(
            "res-{:x}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos()),
            std::process::id()
        );
        match store.try_acquire_offer_reservation_lease(
            &crate::storage::OfferReservationLeaseRequest {
                reservation_id: &reservation_id,
                market_id,
                wallet_id,
                requested_amounts,
                available_amounts,
                lease_seconds: self.lease_seconds,
                now: None,
            },
        )? {
            OfferReservationAcquireOutcome::Acquired => {
                Ok(ReservationAcquireResult::Acquired { reservation_id })
            }
            OfferReservationAcquireOutcome::Rejected(reason) => {
                Ok(ReservationAcquireResult::Rejected { reason })
            }
        }
    }

    pub fn release(&self, reservation_id: &str, release_status: &str) -> SignerResult<()> {
        let store = self.store.lock()?;
        store.release_offer_reservation_lease(reservation_id, release_status)?;
        Ok(())
    }

    pub fn expire_stale(&self) -> SignerResult<u64> {
        let store = self.store.lock()?;
        store.expire_offer_reservation_leases(None)
    }
}

#[derive(Debug, Clone)]
pub enum ReservationAcquireResult {
    Acquired {
        reservation_id: String,
    },
    Rejected {
        reason: OfferReservationRejectReason,
    },
}
