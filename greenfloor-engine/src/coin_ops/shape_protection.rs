//! Ladder-row split protection shared by offer bootstrap and daemon coin ops.
//!
//! ## Primary row invariant
//!
//! Combine-first bootstrap always targets the **largest configured ladder row** size.
//! Preflight deferral and daemon low-watermark split protection use the same primary row
//! (`max` ladder size). Ladder configs with a combine target below the largest rung are
//! unsupported.

use std::collections::{HashMap, HashSet};

use crate::config::LadderEntry;

use super::selection::SpendableCoin;
use super::shape_defer::spendable_amounts_in_base_units;

/// Canonical `(size_base_units, target_count + split_buffer_count)` slot for a ladder row.
#[must_use]
pub fn required_ladder_row_slot(
    size_base_units: i64,
    target_count: i64,
    split_buffer_count: i64,
) -> (i64, i64) {
    (size_base_units, target_count + split_buffer_count)
}

/// Required slot rows from daemon/market [`LadderEntry`] values.
#[must_use]
pub fn required_rows_from_ladder_entries(entries: &[LadderEntry]) -> Vec<(i64, i64)> {
    entries
        .iter()
        .map(|row| {
            required_ladder_row_slot(
                row.size_base_units,
                row.target_count,
                row.split_buffer_count,
            )
        })
        .collect()
}

/// Exact ladder-row counts and protected slot requirements for split-source policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderShapeContext {
    pub ladder_sizes: HashSet<i64>,
    pub protected_slots: HashMap<i64, i64>,
    pub exact_ladder_counts: HashMap<i64, i64>,
}

impl LadderShapeContext {
    /// Build shape context from `(size_base_units, required_count)` rows and spendable amounts.
    #[must_use]
    pub fn from_required_rows(rows: &[(i64, i64)], spendable_amounts_base_units: &[i64]) -> Self {
        let ladder_sizes: Vec<i64> = rows.iter().map(|(size, _)| *size).collect();
        Self {
            ladder_sizes: ladder_sizes.iter().copied().collect(),
            protected_slots: rows.iter().copied().collect(),
            exact_ladder_counts: exact_ladder_coin_counts(
                spendable_amounts_base_units,
                &ladder_sizes,
            ),
        }
    }

    #[must_use]
    pub fn from_sell_ladder_entries(entries: &[LadderEntry]) -> Self {
        Self::from_required_rows(&required_rows_from_ladder_entries(entries), &[])
    }

    /// Largest configured ladder row — the combine-first bootstrap primary row.
    #[must_use]
    pub fn primary_row_size(&self) -> Option<i64> {
        primary_ladder_row_size(&self.ladder_sizes.iter().copied().collect::<Vec<_>>())
    }

    #[must_use]
    pub fn primary_row_satisfied(&self) -> bool {
        let Some(primary_size) = self.primary_row_size() else {
            return false;
        };
        primary_row_satisfied(
            primary_size,
            &self.protected_slots,
            &self.exact_ladder_counts,
        )
    }
}

/// Ladder-aware split-source protection for daemon low-watermark splits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitSourceProtection {
    pub shape: LadderShapeContext,
    pub base_unit_mojo_multiplier: i64,
}

impl SplitSourceProtection {
    #[must_use]
    pub fn from_sell_ladder_entries(
        entries: &[LadderEntry],
        spendable: &[SpendableCoin],
        base_unit_mojo_multiplier: i64,
    ) -> Self {
        Self::from_required_rows(
            &required_rows_from_ladder_entries(entries),
            &spendable_amounts_in_base_units(spendable, base_unit_mojo_multiplier),
            base_unit_mojo_multiplier,
        )
    }

    #[must_use]
    pub fn from_required_rows(
        rows: &[(i64, i64)],
        spendable_amounts_base_units: &[i64],
        base_unit_mojo_multiplier: i64,
    ) -> Self {
        Self {
            shape: LadderShapeContext::from_required_rows(rows, spendable_amounts_base_units),
            base_unit_mojo_multiplier,
        }
    }

    #[must_use]
    pub fn select_spendable_coin<'a>(
        &self,
        spendable: &'a [SpendableCoin],
        required_amount_base_units: i64,
        exclude_coin_ids: &HashSet<String>,
    ) -> Option<&'a SpendableCoin> {
        let multiplier = self.base_unit_mojo_multiplier.max(1);
        let required_mojos = required_amount_base_units.saturating_mul(multiplier);
        let candidates: Vec<SplittableCandidate<'_>> = spendable
            .iter()
            .filter(|coin| {
                !coin.id.is_empty()
                    && !exclude_coin_ids.contains(&coin.id)
                    && coin.amount >= required_mojos
            })
            .map(|coin| SplittableCandidate {
                id: coin.id.as_str(),
                amount_base_units: coin.amount / multiplier,
            })
            .collect();
        let index = select_smallest_non_cannibalizing_index(
            &candidates,
            required_amount_base_units,
            &self.shape,
        )?;
        let selected_id = candidates[index].id;
        spendable.iter().find(|coin| coin.id == selected_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplittableCandidate<'a> {
    pub id: &'a str,
    pub amount_base_units: i64,
}

/// Count spendable coins whose amount exactly matches a configured ladder size.
#[must_use]
pub fn exact_ladder_coin_counts(
    spendable_amounts_base_units: &[i64],
    ladder_sizes: &[i64],
) -> HashMap<i64, i64> {
    let mut counts: HashMap<i64, i64> = ladder_sizes.iter().map(|size| (*size, 0)).collect();
    for amount in spendable_amounts_base_units {
        if let Some(entry) = counts.get_mut(amount) {
            *entry += 1;
        }
    }
    counts
}

/// True when splitting `coin_amount` would consume a protected exact ladder-row coin for a smaller deficit.
#[must_use]
pub fn split_would_cannibalize_protected_row(
    coin_amount: i64,
    total_output_amount: i64,
    ladder_sizes: &HashSet<i64>,
    protected_slots: &HashMap<i64, i64>,
    counts: &HashMap<i64, i64>,
) -> bool {
    if !ladder_sizes.contains(&coin_amount) {
        return false;
    }
    let required = protected_slots.get(&coin_amount).copied().unwrap_or(0);
    if required <= 0 {
        return false;
    }
    let current = counts.get(&coin_amount).copied().unwrap_or(0);
    if current > 0 && current < required {
        return true;
    }
    total_output_amount < coin_amount && current >= required
}

#[must_use]
pub fn primary_ladder_row_size(ladder_sizes: &[i64]) -> Option<i64> {
    ladder_sizes.iter().copied().max()
}

#[must_use]
pub fn primary_row_satisfied(
    primary_size: i64,
    protected_slots: &HashMap<i64, i64>,
    counts: &HashMap<i64, i64>,
) -> bool {
    let required = protected_slots.get(&primary_size).copied().unwrap_or(0);
    if required <= 0 {
        return false;
    }
    counts.get(&primary_size).copied().unwrap_or(0) >= required
}

/// Remaining shape work is strictly below the primary ladder row.
#[must_use]
pub fn remaining_shape_below_primary_row(remaining_total: i64, primary_size: i64) -> bool {
    remaining_total > 0 && remaining_total < primary_size
}

/// Primary row is on-chain; smaller buffer gaps are daemon coin-op scope.
#[must_use]
pub fn defer_sub_primary_shape_to_coin_ops(
    remaining_total: i64,
    primary_size: i64,
    primary_satisfied: bool,
) -> bool {
    primary_satisfied && remaining_shape_below_primary_row(remaining_total, primary_size)
}

/// Index of the smallest candidate that can fund `required_output_base_units` without cannibalizing a protected row.
#[must_use]
pub fn select_smallest_non_cannibalizing_index(
    candidates: &[SplittableCandidate<'_>],
    required_output_base_units: i64,
    ctx: &LadderShapeContext,
) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.amount_base_units >= required_output_base_units)
        .filter(|(_, candidate)| {
            !split_would_cannibalize_protected_row(
                candidate.amount_base_units,
                required_output_base_units,
                &ctx.ladder_sizes,
                &ctx.protected_slots,
                &ctx.exact_ladder_counts,
            )
        })
        .min_by_key(|(_, candidate)| candidate.amount_base_units)
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cannibalizing_satisfied_primary_row() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (100, 1)], &[]);
        let counts = HashMap::from([(100, 1), (10, 2)]);
        let mut ctx = ctx;
        ctx.exact_ladder_counts = counts.clone();
        assert!(split_would_cannibalize_protected_row(
            100,
            10,
            &ctx.ladder_sizes,
            &ctx.protected_slots,
            &counts,
        ));
    }

    #[test]
    fn unified_selector_picks_smallest_eligible_candidate() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (100, 1)], &[100, 50, 10, 10]);
        let candidates = [
            SplittableCandidate {
                id: "combined",
                amount_base_units: 100,
            },
            SplittableCandidate {
                id: "spare",
                amount_base_units: 50,
            },
            SplittableCandidate {
                id: "ten",
                amount_base_units: 10,
            },
        ];
        let index =
            select_smallest_non_cannibalizing_index(&candidates, 20, &ctx).expect("eligible");
        assert_eq!(candidates[index].id, "spare");
    }
}
