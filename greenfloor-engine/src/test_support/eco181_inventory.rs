//! Shared ECO.181 fragmented CAT inventory for combine-cap regression tests.

/// Canonical ECO.181 fragmented inventory as `(coin id, amount)` rows (19 coins, 135 total).
#[must_use]
pub fn eco181_fragmented_inventory_rows() -> Vec<(String, i64)> {
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
