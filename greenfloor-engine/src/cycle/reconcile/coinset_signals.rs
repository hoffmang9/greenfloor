//! Coinset tx signal summary shared by reconcile dispatch paths.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DexieCoinsetSignals {
    pub tx_ids: Vec<String>,
    pub confirmed_tx_ids: Vec<String>,
    pub mempool_tx_ids: Vec<String>,
}

impl DexieCoinsetSignals {
    #[must_use]
    pub fn summary(&self) -> CoinsetSignalSummary {
        CoinsetSignalSummary::from_tx_lists(
            &self.tx_ids,
            &self.confirmed_tx_ids,
            &self.mempool_tx_ids,
        )
    }
}

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

    /// Watch-hit / inventory signal with no concrete spend-bundle id yet.
    #[must_use]
    pub fn mempool_hit() -> Self {
        Self {
            has_tx_ids: true,
            has_confirmed: false,
            has_mempool: true,
        }
    }
}
