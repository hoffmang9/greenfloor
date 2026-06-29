//! Bootstrap plan domain model and coin row helpers.

use super::amounts::BaseUnits;
use super::combine_inputs::BootstrapCombineInputs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerLadderRow {
    pub size_base_units: i64,
    pub target_count: i64,
    pub split_buffer_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderDeficit {
    pub size_base_units: i64,
    pub required_count: i64,
    pub current_count: i64,
}

impl LadderDeficit {
    #[must_use]
    pub fn new(size_base_units: i64, required_count: i64, current_count: i64) -> Self {
        Self {
            size_base_units,
            required_count,
            current_count,
        }
    }

    #[must_use]
    pub fn deficit_count(&self) -> i64 {
        self.required_count - self.current_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCoin {
    pub id: String,
    pub amount: BaseUnits,
}

impl BootstrapCoin {
    /// Coin has a non-empty id and positive amount (eligible for combine input selection).
    #[must_use]
    pub(crate) fn is_spendable(&self) -> bool {
        !self.id.trim().is_empty() && self.amount.get() > 0
    }
}

#[must_use]
pub(crate) fn bootstrap_coin_amounts(coins: &[BootstrapCoin]) -> Vec<i64> {
    coins.iter().map(|coin| coin.amount.get()).collect()
}

#[must_use]
pub(crate) fn spendable_bootstrap_coins(coins: &[BootstrapCoin]) -> Vec<BootstrapCoin> {
    coins
        .iter()
        .filter(|coin| coin.is_spendable())
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapFundingSource {
    SingleCoin { coin_id: String, amount: BaseUnits },
    CombineFirst(BootstrapCombineInputs),
}

#[must_use]
fn funding_source_amount(funding: &BootstrapFundingSource) -> i64 {
    match funding {
        BootstrapFundingSource::SingleCoin { amount, .. } => amount.get(),
        BootstrapFundingSource::CombineFirst(inputs) => inputs.selected_total.get(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPlan {
    pub funding: BootstrapFundingSource,
    pub output_amounts_base_units: Vec<i64>,
    pub total_output_amount: i64,
    /// Leftover base units after shaping (not mojos). Convert before CAT dust checks.
    pub change_amount: i64,
    pub deficits: Vec<LadderDeficit>,
}

impl BootstrapPlan {
    #[must_use]
    pub(crate) fn needs_shape(
        funding: BootstrapFundingSource,
        total_output_amount: i64,
        output_amounts_base_units: Vec<i64>,
        deficits: Vec<LadderDeficit>,
    ) -> Self {
        debug_assert_eq!(
            total_output_amount,
            output_amounts_base_units.iter().sum::<i64>(),
            "total_output_amount must match output_amounts_base_units"
        );
        Self {
            change_amount: funding_source_amount(&funding) - total_output_amount,
            funding,
            output_amounts_base_units,
            total_output_amount,
            deficits,
        }
    }

    #[must_use]
    pub fn requires_combine_first(&self) -> bool {
        matches!(self.funding, BootstrapFundingSource::CombineFirst(_))
    }

    #[must_use]
    pub fn source_coin_id(&self) -> Option<&str> {
        match &self.funding {
            BootstrapFundingSource::SingleCoin { coin_id, .. } => Some(coin_id.as_str()),
            BootstrapFundingSource::CombineFirst(_) => None,
        }
    }

    #[must_use]
    pub fn source_amount(&self) -> i64 {
        funding_source_amount(&self.funding)
    }

    #[must_use]
    pub fn combine_inputs(&self) -> Option<&BootstrapCombineInputs> {
        match &self.funding {
            BootstrapFundingSource::CombineFirst(inputs) => Some(inputs),
            BootstrapFundingSource::SingleCoin { .. } => None,
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
