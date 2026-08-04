use crate::coin_ops::selection::SpendableCoin;

/// Inputs for daemon automatic split-source selection.
#[derive(Debug, Clone, Copy)]
pub struct DaemonAutoSplitParams<'a> {
    pub candidate_spendable: &'a [SpendableCoin],
    pub required_amount_mojos: i64,
    pub canonical_asset_id: &'a str,
    pub combine_input_cap: i64,
    pub allow_combine_prereq: bool,
}

/// Daemon combine-first input set covering a required amount (on-chain mojos).
///
/// Alias of the unified [`crate::coin_ops::shape::CombineInputs`] core type; `selected_total`
/// and `target_amount` are on-chain mojos for this daemon path.
pub type SplitCombinePrereqPlan = crate::coin_ops::shape::CombineInputs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitCoinPlan {
    pub coin_id: String,
    pub selected_amount_mojos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubCatChangeSkipData {
    pub selected_coin_id: String,
    pub selected_amount_mojos: i64,
    pub required_amount_mojos: i64,
    pub remainder_mojos: i64,
    pub minimum_allowed_mojos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitSkipReason {
    NoSpendableMeetsRequired,
    SubCatChange(SubCatChangeSkipData),
}

impl SplitSkipReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoSpendableMeetsRequired => "no_spendable_split_coin_meets_required_amount",
            Self::SubCatChange(_) => "split_would_create_sub_cat_change",
        }
    }
}

/// CLI auto split selection: largest coin or typed skip only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliSplitSelection {
    Coin(SplitCoinPlan),
    Skip(SplitSkipReason),
}

/// Daemon auto split selection: coin, combine prereq, or typed skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitAutoSelectPlan {
    Coin(SplitCoinPlan),
    CombinePrereq(SplitCombinePrereqPlan),
    Skip(SplitSkipReason),
}
