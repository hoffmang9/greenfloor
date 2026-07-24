/// Cancel target for on-chain offer reclaim.
#[derive(Debug, Clone)]
pub enum CancelOfferTarget {
    /// Tracked offer id; lifecycle state is updated on successful submit.
    Tracked { offer_id: String, market_id: String },
    /// Local offer file or bech32; cancel spends without `SQLite` lifecycle updates.
    LocalFile {
        offer_id: String,
        market_id: String,
        offer_text: String,
    },
}

impl CancelOfferTarget {
    #[must_use]
    pub fn offer_id(&self) -> &str {
        match self {
            Self::Tracked { offer_id, .. } | Self::LocalFile { offer_id, .. } => offer_id,
        }
    }

    #[must_use]
    pub fn market_id(&self) -> &str {
        match self {
            Self::Tracked { market_id, .. } | Self::LocalFile { market_id, .. } => market_id,
        }
    }

    #[must_use]
    pub fn normalized_market_id(&self) -> String {
        let market_id = self.market_id().trim();
        if market_id.is_empty() {
            "unknown".to_string()
        } else {
            market_id.to_string()
        }
    }

    #[must_use]
    pub fn offer_text(&self) -> Option<&str> {
        match self {
            Self::Tracked { .. } => None,
            Self::LocalFile { offer_text, .. } => Some(offer_text.as_str()),
        }
    }

    #[must_use]
    pub fn persists_state(&self) -> bool {
        matches!(self, Self::Tracked { .. })
    }
}

/// Per-target cancel result. Soft orchestration failures use `success: false` (see
/// [`super::cancel_offers_on_chain`]); they are not `Err`.
#[derive(Debug, Clone)]
pub struct CancelOfferOutcome {
    pub offer_id: String,
    pub market_id: String,
    /// True when the cancel spend was broadcast successfully.
    pub success: bool,
    pub operation_id: String,
    /// Hard failure detail (build/prepare/broadcast/observe/rollback). Empty on success.
    pub error: String,
}

pub(super) fn outcome(
    target: &CancelOfferTarget,
    market_id: impl Into<String>,
    success: bool,
    operation_id: impl Into<String>,
    error: impl Into<String>,
) -> CancelOfferOutcome {
    CancelOfferOutcome {
        offer_id: target.offer_id().to_string(),
        market_id: market_id.into(),
        success,
        operation_id: operation_id.into(),
        error: error.into(),
    }
}
