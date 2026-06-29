//! Lightweight unit-test builders for bootstrap ladder rows and coins.
//!
//! Use this module for simple `PlannerLadderRow` / `BootstrapCoin` construction in
//! bootstrap unit tests. Scenario inventories and integration fixtures live under
//! `crate::test_support` (for example `eco181_bootstrap_inventory`).

#[cfg(test)]
pub(super) fn ladder_row(size: i64, target: i64, buffer: i64) -> super::PlannerLadderRow {
    super::PlannerLadderRow {
        size_base_units: size,
        target_count: target,
        split_buffer_count: buffer,
    }
}

#[cfg(test)]
pub(super) fn bootstrap_coin(id: &str, amount: i64) -> super::BootstrapCoin {
    super::BootstrapCoin {
        id: id.to_string(),
        amount: super::BaseUnits::new(amount),
    }
}
