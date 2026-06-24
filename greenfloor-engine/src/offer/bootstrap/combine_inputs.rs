//! Bootstrap-local combine input selection (decoupled from coin-op planner types).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCombineInputs {
    pub input_coin_ids: Vec<String>,
    pub selected_total: i64,
    pub target_amount: i64,
    pub exact_match: bool,
    pub cap_applied: bool,
}

impl BootstrapCombineInputs {
    #[must_use]
    pub fn from_coin_ops(prereq: crate::coin_ops::SplitCombinePrereqPlan) -> Self {
        Self {
            input_coin_ids: prereq.input_coin_ids,
            selected_total: prereq.selected_total,
            target_amount: prereq.target_amount,
            exact_match: prereq.exact_match,
            cap_applied: prereq.cap_applied,
        }
    }
}
