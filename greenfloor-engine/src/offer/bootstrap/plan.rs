//! Bootstrap plan domain model and coin row helpers.

use super::amounts::PlanAmount;
use crate::coin_ops::shape::{CombineInputs, ShapeDeficit, ShapeFunding};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerLadderRow {
    /// Plan amount for this ladder clip (mojos on the signer denomination path).
    pub size: i64,
    pub target_count: i64,
    pub split_buffer_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCoin {
    pub id: String,
    pub amount: PlanAmount,
}

#[must_use]
pub(crate) fn bootstrap_coin_amounts(coins: &[BootstrapCoin]) -> Vec<i64> {
    coins.iter().map(|coin| coin.amount.get()).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPlan {
    pub funding: ShapeFunding,
    /// Mixed-split outputs in plan amounts (mojos on the signer denomination path).
    pub output_amounts: Vec<i64>,
    pub total_output_amount: i64,
    /// Leftover plan amount after shaping (same unit as ladder/coins for this plan).
    pub change_amount: i64,
    pub deficits: Vec<ShapeDeficit>,
}

impl BootstrapPlan {
    #[must_use]
    pub(crate) fn needs_shape(
        funding: ShapeFunding,
        total_output_amount: i64,
        output_amounts: Vec<i64>,
        deficits: Vec<ShapeDeficit>,
    ) -> Self {
        debug_assert_eq!(
            total_output_amount,
            output_amounts.iter().sum::<i64>(),
            "total_output_amount must match output_amounts"
        );
        Self {
            change_amount: funding.amount() - total_output_amount,
            funding,
            output_amounts,
            total_output_amount,
            deficits,
        }
    }

    #[must_use]
    pub fn requires_combine_first(&self) -> bool {
        matches!(self.funding, ShapeFunding::CombineFirst(_))
    }

    #[must_use]
    pub fn source_coin_id(&self) -> Option<&str> {
        match &self.funding {
            ShapeFunding::SingleCoin { coin_id, .. } => Some(coin_id.as_str()),
            ShapeFunding::CombineFirst(_) => None,
        }
    }

    #[must_use]
    pub fn source_amount(&self) -> i64 {
        self.funding.amount()
    }

    #[must_use]
    pub fn combine_inputs(&self) -> Option<&CombineInputs> {
        match &self.funding {
            ShapeFunding::CombineFirst(inputs) => Some(inputs),
            ShapeFunding::SingleCoin { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapPlanOutcome {
    Ready,
    NeedsShape(BootstrapPlan),
    CannotFund { total_output_amount: i64 },
    InvalidLadder,
    InvalidCoins,
}

impl BootstrapPlanOutcome {
    /// True when the planner still requires a combine-first funding step.
    #[must_use]
    pub(crate) fn combine_first_pending(&self) -> bool {
        matches!(self, Self::NeedsShape(plan) if plan.requires_combine_first())
    }
}
