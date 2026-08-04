//! Expired / surplus durable maker policy: soft-mark, CAS lease, surplus reclaim plan.
//!
//! Soft-expire phase and ensure `PreferExisting` share this spine. Cancel (ADR 0015) stays
//! separate and only shares spend construction in `offer::reclaim`.

mod lease;
mod mark;
mod plan;

pub use lease::{reclaim_expired_maker_if_unspent, ExpiredMakerLease, ReclaimMakerOutcome};
pub use mark::{mark_listings_soft_expired, restore_stale_maker_claims_synced};
pub(crate) use plan::plan_soft_expire_reclaims;
