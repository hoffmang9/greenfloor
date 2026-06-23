use std::collections::BTreeMap;
use std::path::Path;

use greenfloor_engine::storage::{
    CoinOpLedgerEntry, OfferReservationAcquireOutcome, OfferReservationLeaseRequest, SqliteStore,
};
use rusqlite::Connection;

#[path = "../support/sqlite.rs"]
mod support;

pub use support::open_store;

pub fn acquire_test_reservation_lease(
    store: &SqliteStore,
    reservation_id: &str,
    wallet_id: &str,
    amounts: &BTreeMap<String, i64>,
    lease_seconds: i64,
) {
    assert!(
        matches!(
            store
                .try_acquire_offer_reservation_lease(&OfferReservationLeaseRequest {
                    reservation_id,
                    market_id: "m1",
                    wallet_id,
                    requested_amounts: amounts,
                    available_amounts: amounts,
                    lease_seconds,
                    now: None,
                })
                .expect("try acquire"),
            OfferReservationAcquireOutcome::Acquired
        ),
        "reservation acquire failed for {reservation_id}"
    );
}

pub fn coin_op_entry<'a>(
    market_id: &'a str,
    op_type: &'a str,
    op_count: i64,
    fee_mojos: i64,
    status: &'a str,
    reason: &'a str,
    operation_id: Option<&'a str>,
) -> CoinOpLedgerEntry<'a> {
    CoinOpLedgerEntry {
        market_id,
        op_type,
        op_count,
        fee_mojos,
        status,
        reason,
        operation_id,
    }
}

pub fn raw_conn(path: &Path) -> Connection {
    Connection::open(path).expect("open raw sqlite connection")
}
