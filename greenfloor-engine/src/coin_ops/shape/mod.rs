//! Shared bootstrap + daemon coin-shape planning core (combine-first funding, ladder deficits).
//!
//! Behavior-preserving unification of what were previously separate bootstrap
//! (`offer::bootstrap`) and daemon (`coin_ops::input_selection`) combine/funding
//! implementations. Path-specific policy — fee budgets, gates, defer rules, CLI adapters,
//! and the `plan_coin_ops` batch count scheduler — stays in its original home; only the pure
//! planning core lives here. See `docs/decisions/` for the migration rationale.

mod combine;
mod deficit;
mod funding;
mod types;

pub use combine::{plan_combine_inputs_for_target, plan_ladder_preserving_combine};
pub use deficit::{collect_shape_deficits, protected_slots_for_rows, shape_context_for_rows};
pub use funding::{
    plan_shape_from_deficits, resolve_shape_funding, ShapeFundingOptions, ShapePlan,
    ShapePlanOutcome,
};
pub use types::{
    AmountUnit, CombineInputs, ShapeCoin, ShapeDeficit, ShapeFunding, ShapeFundingResolution,
    ShapeLadderRow,
};
