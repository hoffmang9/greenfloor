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

/// john-deere live inventory mapped to base units (`amount_mojos / 1000`).
#[must_use]
pub fn john_deere_current_inventory_rows() -> Vec<(String, i64)> {
    let mut rows = Vec::with_capacity(17);
    for index in 0..11 {
        rows.push((format!("one_{index}"), 1));
    }
    rows.push(("dust_low".to_string(), 1));
    rows.push(("dust_high".to_string(), 3));
    for index in 0..3 {
        rows.push((format!("ten_{index}"), 10));
    }
    rows.push(("ninety".to_string(), 90));
    rows
}

/// Post-bootstrap inventory: 100 BU primary row plus john-deere remnants.
#[must_use]
pub fn john_deere_after_combine_inventory_rows() -> Vec<(String, i64)> {
    let mut rows = eco181_after_combine_inventory_rows();
    rows.push(("ninety".to_string(), 90));
    rows
}

#[must_use]
pub fn john_deere_current_bootstrap_coins() -> Vec<BootstrapCoin> {
    bootstrap_coins_from_rows(&john_deere_current_inventory_rows())
}

#[must_use]
pub fn john_deere_after_combine_coins() -> Vec<BootstrapCoin> {
    bootstrap_coins_from_rows(&john_deere_after_combine_inventory_rows())
}

/// Deterministic 64-char hex coin id for coinset fixtures (label must be unique per fixture row).
#[must_use]
pub fn eco181_fixture_coin_id(label: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    label.hash(&mut hasher);
    format!("{:064x}", hasher.finish())
}

/// Build coinset `coin_records` JSON for a fixture inventory (amounts in base units).
#[must_use]
pub fn eco181_fixture_coin_records(rows: &[(String, i64)], mojo_multiplier: i64) -> String {
    use crate::test_support::bootstrap_shape::{coin_record_body, coin_records_response};

    let records: Vec<String> = rows
        .iter()
        .map(|(label, amount)| {
            coin_record_body(
                &eco181_fixture_coin_id(label),
                u64::try_from(amount.saturating_mul(mojo_multiplier)).unwrap_or(u64::MAX),
            )
        })
        .collect();
    coin_records_response(&records)
}
