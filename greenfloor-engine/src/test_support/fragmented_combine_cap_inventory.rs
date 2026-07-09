//! Fragmented inventory where unconstrained exact-match selection exceeds combine input cap.

use crate::coin_ops::SpendableCoin;

/// `(coin id, amount)` rows: 19 coins totaling 135 with no single coin ≥ 100.
#[must_use]
pub fn fragmented_combine_cap_inventory_rows() -> Vec<(String, i64)> {
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
    rows.push(("twenty".to_string(), 20));
    rows.push(("sixtyfive".to_string(), 65));
    rows
}

#[must_use]
pub fn fragmented_combine_cap_spendable_coins() -> Vec<SpendableCoin> {
    fragmented_combine_cap_inventory_rows()
        .into_iter()
        .map(|(id, amount)| SpendableCoin::new(id, amount))
        .collect()
}
