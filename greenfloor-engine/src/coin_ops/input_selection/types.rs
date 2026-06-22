#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineInputSelectionMode {
    LargestByAmount,
    ExactAmount,
}

impl CombineInputSelectionMode {
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "largest_by_amount" => Some(Self::LargestByAmount),
            "exact_amount" => Some(Self::ExactAmount),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitCombinePrereqPlan {
    pub input_coin_ids: Vec<String>,
    pub target_amount: i64,
    pub selected_total: i64,
    pub exact_match: bool,
    pub cap_applied: bool,
    pub selected_count_before_cap: usize,
    pub combine_input_cap: i64,
}

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
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoSpendableMeetsRequired => "no_spendable_split_coin_meets_required_amount",
            Self::SubCatChange(_) => "split_would_create_sub_cat_change",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitAutoSelectPlan {
    Coin(SplitCoinPlan),
    CombinePrereq(SplitCombinePrereqPlan),
    Skip(SplitSkipReason),
}
