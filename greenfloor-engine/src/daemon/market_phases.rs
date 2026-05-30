#[derive(Debug, Clone, Default)]
pub struct MarketPhaseMetrics {
    pub cycle_error_count: u64,
    pub strategy_planned_total: u64,
    pub strategy_executed_total: u64,
}
