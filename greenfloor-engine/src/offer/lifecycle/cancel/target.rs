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

/// Per-target cancel result. Soft orchestration failures are [`Self::Failed`] (see
/// [`super::cancel_offers_on_chain`]); they are not `Err`.
#[derive(Debug, Clone)]
pub enum CancelOfferOutcome {
    Submitted {
        offer_id: String,
        market_id: String,
        operation_id: String,
    },
    Failed {
        offer_id: String,
        market_id: String,
        operation_id: String,
        error: String,
    },
}

impl CancelOfferOutcome {
    #[must_use]
    pub fn offer_id(&self) -> &str {
        match self {
            Self::Submitted { offer_id, .. } | Self::Failed { offer_id, .. } => offer_id,
        }
    }

    #[must_use]
    pub fn market_id(&self) -> &str {
        match self {
            Self::Submitted { market_id, .. } | Self::Failed { market_id, .. } => market_id,
        }
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        match self {
            Self::Submitted { operation_id, .. } | Self::Failed { operation_id, .. } => {
                operation_id
            }
        }
    }

    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Submitted { .. })
    }

    #[must_use]
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Submitted { .. } => None,
            Self::Failed { error, .. } => Some(error.as_str()),
        }
    }
}

pub(super) fn submitted(
    target: &CancelOfferTarget,
    market_id: impl Into<String>,
    operation_id: impl Into<String>,
) -> CancelOfferOutcome {
    CancelOfferOutcome::Submitted {
        offer_id: target.offer_id().to_string(),
        market_id: market_id.into(),
        operation_id: operation_id.into(),
    }
}

pub(super) fn failed(
    target: &CancelOfferTarget,
    market_id: impl Into<String>,
    operation_id: impl Into<String>,
    error: impl Into<String>,
) -> CancelOfferOutcome {
    CancelOfferOutcome::Failed {
        offer_id: target.offer_id().to_string(),
        market_id: market_id.into(),
        operation_id: operation_id.into(),
        error: error.into(),
    }
}
