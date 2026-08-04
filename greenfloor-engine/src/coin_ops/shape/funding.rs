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
/// Each variant is a closed set of the funding policies bootstrap and daemon coin ops
/// actually use; the sealed enum documents each call site's policy choice by construction
/// instead of via independently-settable bool flags.
#[derive(Debug, Clone, Copy)]
pub enum ShapeFundingPolicy<'a> {
    /// Bootstrap combine-first funding: ladder base units, ladder-preserving combine retry,
    /// smallest non-cannibalizing single-coin preference, and single-coin-coverage exclusion
    /// for combine eligibility (see `offer::bootstrap::planner::plan_bootstrap_mixed_outputs`).
    Bootstrap {
        combine_input_cap: i64,
        canonical_asset_id: &'a str,
        dust_mojo_multiplier: i64,
    },
    /// Daemon auto-split funding without ladder-row protection: largest-covering single
    /// coin, flat combine-first fallback when `allow_combine`.
    DaemonUnprotected {
        combine_input_cap: i64,
        canonical_asset_id: &'a str,
        allow_combine: bool,
    },
    /// Daemon low-watermark funding with ladder-row cannibalization protection: smallest
    /// non-cannibalizing single coin, flat combine-first fallback when `allow_combine`.
    DaemonProtected {
        combine_input_cap: i64,
        canonical_asset_id: &'a str,
        base_unit_mojo_multiplier: i64,
        allow_combine: bool,
    },
}

/// Combine-first fallback strategy resolved from a [`ShapeFundingPolicy`] variant.
///
/// `LadderPreserving` always implies both ladder-row protection *and*
/// single-coin-coverage exclusion (see [`ShapeFundingPolicy::Bootstrap`], the only variant
/// that currently sets either); `Flat`/`Disabled` set neither (both daemon variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CombineStrategy {
    Disabled,
    Flat,
    LadderPreserving,
}

/// Resolved internal flags for a [`ShapeFundingPolicy`] variant.
struct ShapeFundingFlags<'a> {
    unit: AmountUnit,
    combine_input_cap: i64,
    canonical_asset_id: &'a str,
    combine_strategy: CombineStrategy,
    prefer_smallest_non_cannibalizing: bool,
}

impl<'a> ShapeFundingPolicy<'a> {
    fn flags(&self) -> ShapeFundingFlags<'a> {
        match *self {
            Self::Bootstrap {
                combine_input_cap,
                canonical_asset_id,
                dust_mojo_multiplier,
            } => ShapeFundingFlags {
                unit: AmountUnit::BaseUnits {
                    dust_mojo_multiplier,
                },
                combine_input_cap,
                canonical_asset_id,
                combine_strategy: CombineStrategy::LadderPreserving,
                prefer_smallest_non_cannibalizing: true,
            },
            Self::DaemonUnprotected {
                combine_input_cap,
                canonical_asset_id,
                allow_combine,
            } => ShapeFundingFlags {
                unit: AmountUnit::Mojos {
                    base_unit_mojo_multiplier: 1,
                },
                combine_input_cap,
                canonical_asset_id,
                combine_strategy: if allow_combine {
                    CombineStrategy::Flat
                } else {
                    CombineStrategy::Disabled
                },
                prefer_smallest_non_cannibalizing: false,
            },
            Self::DaemonProtected {
                combine_input_cap,
                canonical_asset_id,
                base_unit_mojo_multiplier,
                allow_combine,
            } => ShapeFundingFlags {
                unit: AmountUnit::Mojos {
                    base_unit_mojo_multiplier,
                },
                combine_input_cap,
                canonical_asset_id,
                combine_strategy: if allow_combine {
                    CombineStrategy::Flat
                } else {
                    CombineStrategy::Disabled
                },
                prefer_smallest_non_cannibalizing: true,
            },
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
/// `policy`, else combine-first when aggregate inventory allows.
///
/// `ladder_shape` must be `Some` whenever `policy` prefers smallest non-cannibalizing
/// selection or protects ladder rows during combine (i.e. [`ShapeFundingPolicy::Bootstrap`]
/// or [`ShapeFundingPolicy::DaemonProtected`]).
///
/// # Panics
///
/// Panics if `ladder_shape` is `None` while `policy` requires a shape context — every such
/// call site must supply one.
#[must_use]
pub fn resolve_shape_funding(
    coins: &[ShapeCoin],
    required_amount: i64,
    ladder_shape: Option<&LadderShapeContext>,
    policy: &ShapeFundingPolicy<'_>,
) -> ShapeFundingResolution {
    let flags = policy.flags();
    let ladder_multiplier = flags.unit.ladder_conversion_multiplier();
    let selected = if flags.prefer_smallest_non_cannibalizing {
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

    if flags.combine_strategy == CombineStrategy::Disabled {
        return ShapeFundingResolution::CannotFund { required_amount };
    }
    let ladder_preserving = flags.combine_strategy == CombineStrategy::LadderPreserving;

    let amounts: Vec<i64> = coins.iter().map(|coin| coin.amount).collect();
    let feasible = if ladder_preserving {
        aggregate_covers_without_single_coin(required_amount, &amounts)
    } else {
        required_amount > 0 && amounts.iter().sum::<i64>() >= required_amount
    };
    if !feasible {
        return ShapeFundingResolution::CannotFund { required_amount };
    }

    let combine = if ladder_preserving {
        let ctx = ladder_shape.expect("ladder_shape required when protect_ladder_rows is set");
        plan_ladder_preserving_combine(
            coins,
            &ctx.protected_slots,
            required_amount,
            flags.combine_input_cap,
            flags.unit.dust_change_mojo_multiplier(),
            flags.canonical_asset_id,
        )
    } else {
        plan_combine_inputs_for_target(coins, required_amount, flags.combine_input_cap)
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
    policy: &ShapeFundingPolicy<'_>,
) -> ShapePlanOutcome {
    let spendable_amounts: Vec<i64> = coins.iter().map(|coin| coin.amount).collect();
    let shape_ctx = shape_context_for_rows(sorted_rows, &spendable_amounts);
    let (deficits, output_amounts) = collect_shape_deficits(sorted_rows, &shape_ctx);
    if deficits.is_empty() {
        return ShapePlanOutcome::Ready;
    }

    let total_output_amount: i64 = output_amounts.iter().sum();
    match resolve_shape_funding(coins, total_output_amount, Some(&shape_ctx), policy) {
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
