//! Bootstrap-local combine input selection (base units only).

use super::amounts::BaseUnits;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCombineInputs {
    pub input_coin_ids: Vec<String>,
    pub selected_total: BaseUnits,
    pub target_amount: BaseUnits,
    pub exact_match: bool,
    pub cap_applied: bool,
}
