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
use super::unit_convert::exact_whole_units_from_mojos;

/// Exact whole ladder-unit amounts from spendable coins (for bucket / protection / ownership).
///
/// Fractional CAT coins (e.g. `10_500` mojos = `10.5` units) are valid inventory but are
/// omitted here — they are not an exact ladder clip.
#[must_use]
pub fn spendable_exact_ladder_unit_amounts(
    spendable: &[SpendableCoin],
    base_unit_mojo_multiplier: i64,
) -> Vec<i64> {
    let multiplier = base_unit_mojo_multiplier.max(1);
    spendable
        .iter()
        .filter_map(|coin| exact_whole_units_from_mojos(coin.amount, multiplier))
        .collect()
}

/// Canonical `(size_base_units, target_count + split_buffer_count)` slot for a ladder row.
#[must_use]
pub fn required_ladder_row_slot(
    size_base_units: i64,
    target_count: i64,
    split_buffer_count: i64,
) -> (i64, i64) {
    (size_base_units, target_count + split_buffer_count)
}

/// Required slot rows from daemon/market [`LadderEntry`] values (target + buffer).
#[must_use]
pub fn required_rows_from_ladder_entries(entries: &[LadderEntry]) -> Vec<(i64, i64)> {
    required_ladder_row_slots(entries.iter().map(|row| {
        (
            row.size_base_units,
            row.target_count,
            row.split_buffer_count,
        )
    }))
}

/// Required `(size, target+buffer)` rows from configured ladder rungs.
#[must_use]
pub fn required_ladder_row_slots(
    rows: impl IntoIterator<Item = (i64, i64, i64)>,
) -> Vec<(i64, i64)> {
    rows.into_iter()
        .map(|(size_base_units, target_count, split_buffer_count)| {
            required_ladder_row_slot(size_base_units, target_count, split_buffer_count)
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
    /// Low-watermark split-source protection: **`target_count` only** (buffer raidable).
    ///
    /// Distinct from [`LadderShapeContext::from_sell_ladder_entries`], which uses
    /// target+buffer for bootstrap / primary-row readiness.
    ///
    /// `inventory_including_watched` must be full-vault exact-clip inventory so
    /// open-offer makers count toward target coverage.
    #[must_use]
    pub fn for_low_watermark_split(
        entries: &[LadderEntry],
        inventory_including_watched: &[SpendableCoin],
        base_unit_mojo_multiplier: i64,
    ) -> Self {
        let target_only_rows = required_ladder_row_slots(entries.iter().filter_map(|row| {
            (row.size_base_units > 0).then_some((
                row.size_base_units,
                row.target_count.max(0),
                0, // buffer raidable for other-rung deficits
            ))
        }));
        Self::from_required_rows(
            &target_only_rows,
            &spendable_exact_ladder_unit_amounts(
                inventory_including_watched,
                base_unit_mojo_multiplier,
            ),
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
        select_smallest_non_cannibalizing_spendable(
            spendable,
            required_amount_base_units,
            self.base_unit_mojo_multiplier,
            exclude_coin_ids,
            &self.shape,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplittableCandidate<'a> {
    pub id: &'a str,
    /// Floored units for funding capacity and smallest-wins ordering (not ladder-clip identity).
    pub funding_units: i64,
    /// Exact ladder clip size when the amount is a clean multiple of the unit multiplier.
    /// `None` for fractional coins: they remain valid funding sources but never count as a
    /// protected ladder row for cannibalization checks.
    pub exact_ladder_units: Option<i64>,
}

impl<'a> SplittableCandidate<'a> {
    /// Build a candidate from on-chain mojos (or any amount scaled by `mojo_multiplier`).
    #[must_use]
    pub fn from_mojos(id: &'a str, amount_mojos: i64, mojo_multiplier: i64) -> Self {
        Self {
            id,
            funding_units: crate::coin_ops::floored_units_from_mojos(amount_mojos, mojo_multiplier),
            exact_ladder_units: crate::coin_ops::exact_whole_units_from_mojos(
                amount_mojos,
                mojo_multiplier,
            ),
        }
    }

    /// Build a candidate already expressed in ladder/plan units (exact clip when positive).
    #[must_use]
    pub fn from_plan_units(id: &'a str, funding_units: i64) -> Self {
        Self {
            id,
            funding_units,
            exact_ladder_units: (funding_units > 0).then_some(funding_units),
        }
    }
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

/// True when splitting `coin_amount` would drop a protected exact ladder row below its slot count.
///
/// Excess exact clips (`current > required`) may fund smaller-rung deficits.
#[must_use]
pub fn split_would_cannibalize_protected_row(
    coin_amount: i64,
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
    if current <= 0 {
        return false;
    }
    // i64::saturating_sub does not clamp at zero.
    current.saturating_sub(1).max(0) < required
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
        .filter(|(_, candidate)| candidate.funding_units >= required_output_base_units)
        .filter(|(_, candidate)| {
            candidate.exact_ladder_units.is_none_or(|clip_units| {
                !split_would_cannibalize_protected_row(
                    clip_units,
                    &ctx.ladder_sizes,
                    &ctx.protected_slots,
                    &ctx.exact_ladder_counts,
                )
            })
        })
        .min_by_key(|(_, candidate)| candidate.funding_units)
        .map(|(index, _)| index)
}

/// ID of the smallest candidate that can fund `required_output_base_units` without cannibalizing a protected row.
#[must_use]
pub fn select_smallest_non_cannibalizing_candidate_id<'a>(
    candidates: &'a [SplittableCandidate<'_>],
    required_output_base_units: i64,
    ctx: &LadderShapeContext,
) -> Option<&'a str> {
    let index =
        select_smallest_non_cannibalizing_index(candidates, required_output_base_units, ctx)?;
    Some(candidates[index].id)
}

/// Smallest non-cannibalizing spendable coin meeting `required_amount_base_units`.
#[must_use]
pub fn select_smallest_non_cannibalizing_spendable<'a>(
    spendable: &'a [SpendableCoin],
    required_amount_base_units: i64,
    base_unit_mojo_multiplier: i64,
    exclude_coin_ids: &HashSet<String>,
    ctx: &LadderShapeContext,
) -> Option<&'a SpendableCoin> {
    let multiplier = base_unit_mojo_multiplier.max(1);
    let required_mojos = required_amount_base_units.saturating_mul(multiplier);
    let candidates: Vec<SplittableCandidate<'_>> = spendable
        .iter()
        .filter(|coin| {
            !coin.id.is_empty()
                && !exclude_coin_ids.contains(&coin.id)
                && coin.amount >= required_mojos
        })
        .map(|coin| SplittableCandidate::from_mojos(coin.id.as_str(), coin.amount, multiplier))
        .collect();
    let selected_id = select_smallest_non_cannibalizing_candidate_id(
        candidates.as_slice(),
        required_amount_base_units,
        ctx,
    )?;
    spendable.iter().find(|coin| coin.id == selected_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_sell_ladder_entries_builds_shape_context() {
        use crate::config::LadderEntry;

        let entries = vec![LadderEntry {
            size_base_units: 10,
            target_count: 2,
            split_buffer_count: 1,
            combine_when_excess_factor: 2.0,
        }];
        let ctx = LadderShapeContext::from_sell_ladder_entries(&entries);
        assert_eq!(ctx.primary_row_size(), Some(10));
        assert!(!ctx.primary_row_satisfied());
    }

    #[test]
    fn primary_row_not_satisfied_when_required_slots_missing() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 0)], &[]);
        assert!(!ctx.primary_row_satisfied());
    }

    #[test]
    fn cannibalization_skips_when_count_is_zero() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3)], &[]);
        assert!(!split_would_cannibalize_protected_row(
            10,
            &ctx.ladder_sizes,
            &ctx.protected_slots,
            &ctx.exact_ladder_counts,
        ));
    }

    #[test]
    fn spendable_selector_picks_smallest_non_cannibalizing_coin() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (100, 1)], &[100, 50]);
        let spendable = vec![
            SpendableCoin::new("combined".to_string(), 100_000),
            SpendableCoin::new("spare".to_string(), 50_000),
        ];
        let selected = select_smallest_non_cannibalizing_spendable(
            &spendable,
            20,
            1_000,
            &HashSet::new(),
            &ctx,
        )
        .expect("eligible spendable");
        assert_eq!(selected.id, "spare");
    }

    #[test]
    fn spendable_selector_accepts_fractional_cat_funding_coin() {
        // 10.5 CAT (10_500 mojos) covers a 10-unit request and is not an exact ladder clip,
        // so it must remain selectable even when a protected size-10 row is underfilled.
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (100, 1)], &[10, 10]);
        let spendable = vec![SpendableCoin::new("frac".to_string(), 10_500)];
        let selected = select_smallest_non_cannibalizing_spendable(
            &spendable,
            10,
            1_000,
            &HashSet::new(),
            &ctx,
        )
        .expect("fractional CAT coin should fund");
        assert_eq!(selected.id, "frac");
        assert_eq!(selected.amount, 10_500);
    }

    #[test]
    fn detects_cannibalizing_satisfied_primary_row() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (100, 1)], &[]);
        let counts = HashMap::from([(100, 1), (10, 2)]);
        let mut ctx = ctx;
        ctx.exact_ladder_counts = counts.clone();
        assert!(split_would_cannibalize_protected_row(
            100,
            &ctx.ladder_sizes,
            &ctx.protected_slots,
            &counts,
        ));
    }

    #[test]
    fn allows_splitting_excess_exact_clip_above_target() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (25, 1)], &[25, 25, 10, 10]);
        assert!(!split_would_cannibalize_protected_row(
            25,
            &ctx.ladder_sizes,
            &ctx.protected_slots,
            &ctx.exact_ladder_counts,
        ));
        let spendable = vec![
            SpendableCoin::new("locked_25".to_string(), 25_000),
            SpendableCoin::new("free_25".to_string(), 25_000),
            SpendableCoin::new("frac".to_string(), 10_300),
        ];
        let selected = select_smallest_non_cannibalizing_spendable(
            &spendable,
            20,
            1_000,
            &HashSet::from(["locked_25".to_string()]),
            &ctx,
        )
        .expect("excess size-25 should fund size-10 deficit");
        assert_eq!(selected.id, "free_25");
    }

    #[test]
    fn unified_selector_picks_smallest_eligible_candidate() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (100, 1)], &[100, 50, 10, 10]);
        let candidates = [
            SplittableCandidate::from_plan_units("combined", 100),
            SplittableCandidate::from_plan_units("spare", 50),
            SplittableCandidate::from_plan_units("ten", 10),
        ];
        let index =
            select_smallest_non_cannibalizing_index(&candidates, 20, &ctx).expect("eligible");
        assert_eq!(candidates[index].id, "spare");
    }

    #[test]
    fn unified_selector_prefers_fractional_over_cannibalizing_exact_clip() {
        let ctx = LadderShapeContext::from_required_rows(&[(10, 3), (100, 1)], &[10, 10]);
        let candidates = [
            SplittableCandidate::from_mojos("exact-ten", 10_000, 1_000),
            SplittableCandidate::from_mojos("frac", 10_500, 1_000),
        ];
        let index =
            select_smallest_non_cannibalizing_index(&candidates, 10, &ctx).expect("eligible");
        assert_eq!(candidates[index].id, "frac");
        assert_eq!(candidates[index].exact_ladder_units, None);
    }
}
