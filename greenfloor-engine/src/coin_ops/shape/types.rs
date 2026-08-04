//! Unit-tagged types shared by bootstrap and daemon coin-shape planning.
//!
//! All planning in [`super`] is unit-agnostic: callers pick [`AmountUnit`] to say whether
//! `amount` fields are ladder base units (bootstrap) or on-chain mojos (daemon coin ops).

/// Which unit system amounts use for a shape planning call.
///
/// `Mojos`' `base_unit_mojo_multiplier` converts ladder base-unit row sizes to mojos for
/// cannibalization checks and combine dust-change math; `BaseUnits` amounts are already
/// ladder base units, so the effective multiplier is always `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountUnit {
    BaseUnits,
    Mojos { base_unit_mojo_multiplier: i64 },
}

impl AmountUnit {
    #[must_use]
    pub(super) fn base_unit_mojo_multiplier(self) -> i64 {
        match self {
            Self::BaseUnits => 1,
            Self::Mojos {
                base_unit_mojo_multiplier,
            } => base_unit_mojo_multiplier.max(1),
        }
    }
}

/// One coin row for shape planning (unit system chosen by caller via [`AmountUnit`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeCoin {
    pub id: String,
    pub amount: i64,
}

impl ShapeCoin {
    #[must_use]
    pub fn new(id: impl Into<String>, amount: i64) -> Self {
        Self {
            id: id.into(),
            amount,
        }
    }

    /// Coin has a non-empty id and positive amount (eligible for shape selection).
    #[must_use]
    pub(super) fn is_spendable(&self) -> bool {
        !self.id.trim().is_empty() && self.amount > 0
    }
}

/// Combine-first inputs selected to cover a target amount (unit per caller's [`AmountUnit`]).
///
/// Unifies the former `offer::bootstrap::BootstrapCombineInputs` (base units) and
/// `coin_ops::SplitCombinePrereqPlan` (mojos), which are now aliases of this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombineInputs {
    pub input_coin_ids: Vec<String>,
    pub selected_total: i64,
    pub target_amount: i64,
    pub exact_match: bool,
    pub cap_applied: bool,
    pub selected_count_before_cap: usize,
    pub combine_input_cap: i64,
}

/// Single-coin or combine-first funding resolved by [`super::funding::resolve_shape_funding`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeFunding {
    SingleCoin { coin_id: String, amount: i64 },
    CombineFirst(CombineInputs),
}

impl ShapeFunding {
    #[must_use]
    pub fn amount(&self) -> i64 {
        match self {
            Self::SingleCoin { amount, .. } => *amount,
            Self::CombineFirst(inputs) => inputs.selected_total,
        }
    }
}

/// Outcome of [`super::funding::resolve_shape_funding`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeFundingResolution {
    Funded(ShapeFunding),
    CannotFund { required_amount: i64 },
}

/// One configured ladder row for shape deficit planning (`coin_ops::shape`-generic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeLadderRow {
    pub size: i64,
    pub target_count: i64,
    pub split_buffer_count: i64,
}

/// Deficit between required and current exact-amount coin counts for one ladder row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeDeficit {
    pub size: i64,
    pub required_count: i64,
    pub current_count: i64,
}

impl ShapeDeficit {
    #[must_use]
    pub fn deficit_count(&self) -> i64 {
        self.required_count - self.current_count
    }
}
