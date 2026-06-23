//! Coinset tx signal summary shared by reconcile dispatch paths.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoinsetSignalSummary {
    pub has_tx_ids: bool,
    pub has_confirmed: bool,
    pub has_mempool: bool,
}

impl CoinsetSignalSummary {
    #[must_use]
    pub fn from_tx_lists(
        coinset_tx_ids: &[String],
        coinset_confirmed_tx_ids: &[String],
        coinset_mempool_tx_ids: &[String],
    ) -> Self {
        Self {
            has_tx_ids: !coinset_tx_ids.is_empty(),
            has_confirmed: !coinset_confirmed_tx_ids.is_empty(),
            has_mempool: !coinset_mempool_tx_ids.is_empty(),
        }
    }
}
