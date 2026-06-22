use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferReservationLeaseRow {
    pub reservation_id: String,
    pub market_id: String,
    pub wallet_id: String,
    pub asset_id: String,
    pub amount: i64,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferReservationLeaseRequest<'a> {
    pub reservation_id: &'a str,
    pub market_id: &'a str,
    pub wallet_id: &'a str,
    pub requested_amounts: &'a BTreeMap<String, i64>,
    pub available_amounts: &'a BTreeMap<String, i64>,
    pub lease_seconds: i64,
    pub now: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfferReservationRejectReason {
    EmptyRequest,
    InsufficientCapacity {
        asset_id: String,
        available: i64,
        reserved: i64,
        needed: i64,
    },
}

impl OfferReservationRejectReason {
    #[must_use]
    pub fn is_insufficient_capacity(&self) -> bool {
        matches!(self, Self::InsufficientCapacity { .. })
    }
}

impl fmt::Display for OfferReservationRejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequest => write!(f, "reservation_empty_request"),
            Self::InsufficientCapacity {
                asset_id,
                available,
                reserved,
                needed,
            } => write!(
                f,
                "reservation_insufficient_{asset_id}:available={available}:reserved={reserved}:needed={needed}"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfferReservationAcquireOutcome {
    Acquired,
    Rejected(OfferReservationRejectReason),
}

pub(super) struct NormalizedReservationLeaseRequest<'a> {
    pub(super) reservation_id: &'a str,
    pub(super) market_id: &'a str,
    pub(super) wallet_id: &'a str,
    pub(super) normalized_requests: BTreeMap<String, i64>,
    pub(super) normalized_available: BTreeMap<String, i64>,
    pub(super) now_iso: String,
    pub(super) expires_at_iso: String,
}

impl<'a> OfferReservationLeaseRequest<'a> {
    pub(super) fn try_normalize(
        &self,
    ) -> SignerResult<Result<NormalizedReservationLeaseRequest<'a>, OfferReservationRejectReason>>
    {
        validate_reservation_id_and_lease(self.reservation_id, self.lease_seconds)?;
        let normalized_requests = positive_asset_amounts(self.requested_amounts);
        if normalized_requests.is_empty() {
            return Ok(Err(OfferReservationRejectReason::EmptyRequest));
        }
        let normalized_available = positive_asset_amounts(self.available_amounts);
        let (now_iso, expires_at_iso) =
            lease_window_iso(self.now.unwrap_or_else(Utc::now), self.lease_seconds);
        Ok(Ok(NormalizedReservationLeaseRequest {
            reservation_id: self.reservation_id,
            market_id: self.market_id,
            wallet_id: self.wallet_id,
            normalized_requests,
            normalized_available,
            now_iso,
            expires_at_iso,
        }))
    }
}

pub(super) fn normalize_asset_id(asset_id: &str) -> String {
    asset_id.trim().to_ascii_lowercase()
}

pub(super) fn validate_reservation_id_and_lease(
    reservation_id: &str,
    lease_seconds: i64,
) -> SignerResult<()> {
    if reservation_id.trim().is_empty() {
        return Err(SignerError::Other("reservation_id is required".to_string()));
    }
    if lease_seconds <= 0 {
        return Err(SignerError::Other("lease_seconds must be > 0".to_string()));
    }
    Ok(())
}

pub(super) fn positive_asset_amounts(source: &BTreeMap<String, i64>) -> BTreeMap<String, i64> {
    source
        .iter()
        .filter(|&(_, amount)| *amount > 0)
        .map(|(asset_id, amount)| (normalize_asset_id(asset_id), *amount))
        .collect()
}

pub(super) fn lease_window_iso(now: DateTime<Utc>, lease_seconds: i64) -> (String, String) {
    let now_iso = now.to_rfc3339();
    let expires_at_iso = (now + chrono::Duration::seconds(lease_seconds)).to_rfc3339();
    (now_iso, expires_at_iso)
}

pub(super) fn first_insufficient_asset(
    requests: &BTreeMap<String, i64>,
    available: &BTreeMap<String, i64>,
    reserved: &BTreeMap<String, i64>,
) -> Option<OfferReservationRejectReason> {
    for (asset_id, amount) in requests {
        let available = available.get(asset_id).copied().unwrap_or(0);
        let already_reserved = reserved.get(asset_id).copied().unwrap_or(0);
        if available - already_reserved < *amount {
            return Some(OfferReservationRejectReason::InsufficientCapacity {
                asset_id: asset_id.clone(),
                available,
                reserved: already_reserved,
                needed: *amount,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_normalize_rejects_empty_request() {
        let requested = BTreeMap::default();
        let available = BTreeMap::default();
        let request = OfferReservationLeaseRequest {
            reservation_id: "res-empty",
            market_id: "m1",
            wallet_id: "vault-1",
            requested_amounts: &requested,
            available_amounts: &available,
            lease_seconds: 120,
            now: None,
        };

        let outcome = request.try_normalize().expect("normalize");

        assert!(matches!(
            outcome,
            Err(OfferReservationRejectReason::EmptyRequest)
        ));
    }

    #[test]
    fn positive_asset_amounts_filters_zero_and_negative_and_lowercases() {
        let mut source = BTreeMap::default();
        source.insert("XCH".to_string(), 100);
        source.insert("  Cat-1  ".to_string(), 250);
        source.insert("ignored".to_string(), 0);
        source.insert("negative".to_string(), -5);

        let out = positive_asset_amounts(&source);

        assert_eq!(out.get("xch"), Some(&100));
        assert_eq!(out.get("cat-1"), Some(&250));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn positive_asset_amounts_empty_when_no_positive_entries() {
        let mut source = BTreeMap::default();
        source.insert("xch".to_string(), 0);
        assert!(positive_asset_amounts(&source).is_empty());
    }

    #[test]
    fn first_insufficient_asset_none_when_capacity_sufficient() {
        let mut requests = BTreeMap::default();
        requests.insert("xch".to_string(), 100);
        let mut available = BTreeMap::default();
        available.insert("xch".to_string(), 200);
        let mut reserved = BTreeMap::default();
        reserved.insert("xch".to_string(), 50);

        assert!(first_insufficient_asset(&requests, &available, &reserved).is_none());
    }

    #[test]
    fn first_insufficient_asset_reports_first_shortfall() {
        let mut requests = BTreeMap::default();
        requests.insert("asset".to_string(), 100);
        requests.insert("xch".to_string(), 20);
        let mut available = BTreeMap::default();
        available.insert("asset".to_string(), 50);
        available.insert("xch".to_string(), 40);

        let reason = first_insufficient_asset(&requests, &available, &BTreeMap::default())
            .expect("shortfall");

        assert_eq!(
            reason,
            OfferReservationRejectReason::InsufficientCapacity {
                asset_id: "asset".to_string(),
                available: 50,
                reserved: 0,
                needed: 100,
            }
        );
        assert!(reason
            .to_string()
            .contains("reservation_insufficient_asset"));
    }

    #[test]
    fn first_insufficient_asset_accounts_for_existing_reservations() {
        let mut requests = BTreeMap::default();
        requests.insert("xch".to_string(), 60);
        let mut available = BTreeMap::default();
        available.insert("xch".to_string(), 100);
        let mut reserved = BTreeMap::default();
        reserved.insert("xch".to_string(), 50);

        let reason = first_insufficient_asset(&requests, &available, &reserved).expect("shortfall");
        assert_eq!(
            reason,
            OfferReservationRejectReason::InsufficientCapacity {
                asset_id: "xch".to_string(),
                available: 100,
                reserved: 50,
                needed: 60,
            }
        );
    }
}
