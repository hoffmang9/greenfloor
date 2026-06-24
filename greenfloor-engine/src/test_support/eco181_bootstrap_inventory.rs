//! ECO.181-style fragmented bootstrap inventory for ladder/combine tests.

use crate::offer::bootstrap::{BaseUnits, BootstrapCoin, PlannerLadderRow};

fn bootstrap_coins_from_rows(rows: &[(String, i64)]) -> Vec<BootstrapCoin> {
    rows.iter()
        .map(|(id, amount)| BootstrapCoin {
            id: id.clone(),
            amount: BaseUnits::new(*amount),
        })
        .collect()
}

/// `(coin id, amount)` rows: 19 coins totaling 135 with no single coin ≥ 100.
#[must_use]
pub fn eco181_bootstrap_inventory_rows() -> Vec<(String, i64)> {
    let mut rows = Vec::with_capacity(19);
    for index in 0..11 {
        rows.push((format!("one_{index}"), 1));
    }
    for index in 0..3 {
        rows.push((format!("three_{index}"), 3));
    }
    for index in 0..3 {
        rows.push((format!("ten_{index}"), 10));
    }
    rows.push(("five".to_string(), 5));
    rows.push(("eighty".to_string(), 80));
    rows
}

#[must_use]
pub fn eco181_after_combine_inventory_rows() -> Vec<(String, i64)> {
    let mut rows = Vec::with_capacity(12);
    for index in 0..9 {
        rows.push((format!("one_{index}"), 1));
    }
    rows.push(("ten_0".to_string(), 10));
    rows.push(("ten_1".to_string(), 10));
    rows.push(("combined_100".to_string(), 100));
    rows
}

#[must_use]
pub fn eco181_bootstrap_ladder() -> Vec<PlannerLadderRow> {
    vec![
        PlannerLadderRow {
            size_base_units: 1,
            target_count: 5,
            split_buffer_count: 1,
        },
        PlannerLadderRow {
            size_base_units: 10,
            target_count: 2,
            split_buffer_count: 1,
        },
        PlannerLadderRow {
            size_base_units: 100,
            target_count: 1,
            split_buffer_count: 0,
        },
    ]
}

#[must_use]
pub fn eco181_bootstrap_coins() -> Vec<BootstrapCoin> {
    bootstrap_coins_from_rows(&eco181_bootstrap_inventory_rows())
}

#[must_use]
pub fn eco181_after_combine_coins() -> Vec<BootstrapCoin> {
    bootstrap_coins_from_rows(&eco181_after_combine_inventory_rows())
}
