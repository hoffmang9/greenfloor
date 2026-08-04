//! Shared single-coin / combine-first funding resolution for bootstrap and daemon coin ops.

use super::combine::{plan_combine_inputs_for_target, plan_ladder_preserving_combine};
use super::deficit::{collect_shape_deficits, shape_context_for_rows};
use super::types::{
    AmountUnit, ShapeCoin, ShapeDeficit, ShapeFunding, ShapeFundingResolution, ShapeLadderRow,
};
use crate::coin_ops::aggregate_covers_without_single_coin;
use crate::coin_ops::shape_protection::{
    select_smallest_non_cannibalizing_candidate_id, LadderShapeContext, SplittableCandidate,
};

/// Explicit funding-resolution policy for [`resolve_shape_funding`] / [`plan_shape_from_deficits`].
///
/// Every field is required so call sites document their policy choice rather than relying on
/// defaults (bootstrap and daemon intentionally choose different values for every field).
#[allow(clippy::struct_excessive_bools)] // Explicit policy flags; each documents a call-site choice.
#[derive(Debug, Clone, Copy)]
pub struct ShapeFundingOptions<'a> {
    pub unit: AmountUnit,
    pub combine_input_cap: i64,
    pub canonical_asset_id: &'a str,
    /// Combine-first fallback gate. When `false`, the combine branch is skipped entirely
    /// (no cap trick required) — used by daemon retry attempts that disable combine after
    /// a first pass already failed.
    pub allow_combine: bool,
    /// Ladder-preserving combine retry (partition + release-smallest-protected-first + dust
    /// guard). Bootstrap uses `true`; daemon (protected or not) uses `false` (flat combine,
    /// dust checked by the caller after selection).
    pub protect_ladder_rows: bool,
    /// Single-coin selection heuristic: smallest non-cannibalizing coin (bootstrap, daemon
    /// low-watermark split) vs largest available coin (daemon default / unprotected).
    pub prefer_smallest_non_cannibalizing: bool,
    /// Combine eligibility gate: require aggregate coverage *and* that no single coin
    /// already covers the target (bootstrap). Daemon paths only require aggregate coverage,
    /// since they already know single-coin selection just failed.
    pub require_no_single_coin_covers: bool,
}

impl<'a> ShapeFundingOptions<'a> {
    /// Bootstrap combine-first funding: ladder base units, ladder-preserving combine retry,
    /// smallest non-cannibalizing single-coin preference, and single-coin-coverage exclusion
    /// for combine eligibility (see `offer::bootstrap::planner::plan_bootstrap_mixed_outputs`).
    #[must_use]
    pub fn bootstrap(
        combine_input_cap: i64,
        canonical_asset_id: &'a str,
        dust_mojo_multiplier: i64,
    ) -> Self {
        Self {
            unit: AmountUnit::BaseUnits {
                dust_mojo_multiplier,
            },
            combine_input_cap,
            canonical_asset_id,
            allow_combine: true,
            protect_ladder_rows: true,
            prefer_smallest_non_cannibalizing: true,
            require_no_single_coin_covers: true,
        }
    }

    /// Daemon auto-split funding without ladder-row protection: largest-covering single
    /// coin, flat combine-first fallback when `allow_combine`.
    #[must_use]
    pub fn daemon_unprotected(
        combine_input_cap: i64,
        canonical_asset_id: &'a str,
        allow_combine: bool,
    ) -> Self {
        Self {
            unit: AmountUnit::Mojos {
                base_unit_mojo_multiplier: 1,
            },
            combine_input_cap,
            canonical_asset_id,
            allow_combine,
            protect_ladder_rows: false,
            prefer_smallest_non_cannibalizing: false,
            require_no_single_coin_covers: false,
        }
    }

    /// Daemon low-watermark funding with ladder-row cannibalization protection: smallest
    /// non-cannibalizing single coin, flat combine-first fallback when `allow_combine`.
    #[must_use]
    pub fn daemon_protected(
        combine_input_cap: i64,
        canonical_asset_id: &'a str,
        base_unit_mojo_multiplier: i64,
        allow_combine: bool,
    ) -> Self {
        Self {
            unit: AmountUnit::Mojos {
                base_unit_mojo_multiplier,
            },
            combine_input_cap,
            canonical_asset_id,
            allow_combine,
            protect_ladder_rows: false,
            prefer_smallest_non_cannibalizing: true,
            require_no_single_coin_covers: false,
        }
    }
}

fn select_largest_shape_coin(coins: &[ShapeCoin], required_amount: i64) -> Option<&ShapeCoin> {
    coins
        .iter()
        .filter(|coin| !coin.id.is_empty() && coin.amount >= required_amount)
        .max_by_key(|coin| coin.amount)
}

/// Smallest coin that funds `required_amount` (in `options` units) without cannibalizing a
/// protected ladder row. Converts to ladder base units via `mojo_multiplier` exactly like the
/// historical `SplitSourceProtection` daemon path (integer division, so `mojo_multiplier > 1`
/// callers inherit the same base-unit rounding as before).
fn select_smallest_non_cannibalizing_shape_coin<'a>(
    coins: &'a [ShapeCoin],
    required_amount: i64,
    mojo_multiplier: i64,
    ctx: &LadderShapeContext,
) -> Option<&'a ShapeCoin> {
    let multiplier = mojo_multiplier.max(1);
    let required_base_units = required_amount / multiplier;
    let required_amount_floor = required_base_units.saturating_mul(multiplier);
    let candidates: Vec<SplittableCandidate<'_>> = coins
        .iter()
        .filter(|coin| coin.is_spendable() && coin.amount >= required_amount_floor)
        .map(|coin| SplittableCandidate {
            id: coin.id.as_str(),
            amount_base_units: coin.amount / multiplier,
        })
        .collect();
    let selected_id =
        select_smallest_non_cannibalizing_candidate_id(&candidates, required_base_units, ctx)?;
    coins.iter().find(|coin| coin.id == selected_id)
}

/// Resolve funding for `required_amount` from `coins`: prefer a single covering coin per
/// `options`, else combine-first when aggregate inventory allows.
///
/// `ladder_shape` must be `Some` whenever `options.prefer_smallest_non_cannibalizing` or
/// `options.protect_ladder_rows` is set.
///
/// # Panics
///
/// Panics if `ladder_shape` is `None` while `options.prefer_smallest_non_cannibalizing` or
/// `options.protect_ladder_rows` is `true` — every call site must supply a shape context for
/// those policies.
#[must_use]
pub fn resolve_shape_funding(
    coins: &[ShapeCoin],
    required_amount: i64,
    ladder_shape: Option<&LadderShapeContext>,
    options: &ShapeFundingOptions<'_>,
) -> ShapeFundingResolution {
    let ladder_multiplier = options.unit.ladder_conversion_multiplier();
    let selected = if options.prefer_smallest_non_cannibalizing {
        let ctx = ladder_shape
            .expect("ladder_shape required when prefer_smallest_non_cannibalizing is set");
        select_smallest_non_cannibalizing_shape_coin(coins, required_amount, ladder_multiplier, ctx)
    } else {
        select_largest_shape_coin(coins, required_amount.max(0))
    };
    if let Some(coin) = selected {
        return ShapeFundingResolution::Funded(ShapeFunding::SingleCoin {
            coin_id: coin.id.clone(),
            amount: coin.amount,
        });
    }

    if !options.allow_combine {
        return ShapeFundingResolution::CannotFund { required_amount };
    }

    let amounts: Vec<i64> = coins.iter().map(|coin| coin.amount).collect();
    let feasible = if options.require_no_single_coin_covers {
        aggregate_covers_without_single_coin(required_amount, &amounts)
    } else {
        required_amount > 0 && amounts.iter().sum::<i64>() >= required_amount
    };
    if !feasible {
        return ShapeFundingResolution::CannotFund { required_amount };
    }

    let combine = if options.protect_ladder_rows {
        let ctx = ladder_shape.expect("ladder_shape required when protect_ladder_rows is set");
        plan_ladder_preserving_combine(
            coins,
            &ctx.protected_slots,
            required_amount,
            options.combine_input_cap,
            options.unit.dust_change_mojo_multiplier(),
            options.canonical_asset_id,
        )
    } else {
        plan_combine_inputs_for_target(coins, required_amount, options.combine_input_cap)
    };
    match combine {
        Some(inputs) => ShapeFundingResolution::Funded(ShapeFunding::CombineFirst(inputs)),
        None => ShapeFundingResolution::CannotFund { required_amount },
    }
}

/// One-shot mixed-output shape plan resolved from ladder deficits (bootstrap combine-first
/// core). Daemon callers plan per-bucket via `coin_ops::plan_coin_ops` (batch scheduler) and
/// call [`resolve_shape_funding`] directly for a single required amount instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapePlan {
    pub funding: ShapeFunding,
    pub output_amounts: Vec<i64>,
    pub total_output_amount: i64,
    pub deficits: Vec<ShapeDeficit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapePlanOutcome {
    Ready,
    NeedsShape(ShapePlan),
    CannotFund { required_amount: i64 },
}

/// Build a one-shot mixed-output shape plan from ladder deficits. `sorted_rows` must be
/// sorted by size (callers already sort before invoking, matching historical bootstrap order).
#[must_use]
pub fn plan_shape_from_deficits(
    sorted_rows: &[ShapeLadderRow],
    coins: &[ShapeCoin],
    options: &ShapeFundingOptions<'_>,
) -> ShapePlanOutcome {
    let spendable_amounts: Vec<i64> = coins.iter().map(|coin| coin.amount).collect();
    let shape_ctx = shape_context_for_rows(sorted_rows, &spendable_amounts);
    let (deficits, output_amounts) = collect_shape_deficits(sorted_rows, &shape_ctx);
    if deficits.is_empty() {
        return ShapePlanOutcome::Ready;
    }

    let total_output_amount: i64 = output_amounts.iter().sum();
    match resolve_shape_funding(coins, total_output_amount, Some(&shape_ctx), options) {
        ShapeFundingResolution::Funded(funding) => ShapePlanOutcome::NeedsShape(ShapePlan {
            funding,
            output_amounts,
            total_output_amount,
            deficits,
        }),
        ShapeFundingResolution::CannotFund { required_amount } => {
            ShapePlanOutcome::CannotFund { required_amount }
        }
    }
}
