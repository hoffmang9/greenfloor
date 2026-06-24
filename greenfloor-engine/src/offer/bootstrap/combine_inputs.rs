//! Bootstrap-local combine input selection (base units only).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCombineInputs {
    pub input_coin_ids: Vec<String>,
    /// Sum of selected input coin amounts in ladder base units.
    pub selected_total: i64,
    /// Combine output size in ladder base units (vault submit scales by mojo multiplier).
    pub target_amount: i64,
    pub exact_match: bool,
    pub cap_applied: bool,
}
