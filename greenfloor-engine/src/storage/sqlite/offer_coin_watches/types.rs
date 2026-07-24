//! Watch kinds and match aggregates for durable offer coin/p2 watches.

use super::super::OfferStateListRow;

/// Stored watch row kind (`offer_coin_watches.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WatchKind {
    Coin,
    P2,
}

impl WatchKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Coin => "coin",
            Self::P2 => "p2",
        }
    }

    pub(crate) fn parse(kind: &str) -> Option<Self> {
        match kind {
            "coin" => Some(Self::Coin),
            "p2" => Some(Self::P2),
            _ => None,
        }
    }
}

/// How durable watch rows matched observed WS keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMatchKind {
    Coin,
    P2,
    Both,
}

impl WatchMatchKind {
    #[must_use]
    pub const fn includes_coin(self) -> bool {
        matches!(self, Self::Coin | Self::Both)
    }

    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        if self == other {
            self
        } else {
            Self::Both
        }
    }

    pub(crate) fn from_watch_kind(kind: WatchKind) -> Self {
        match kind {
            WatchKind::Coin => Self::Coin,
            WatchKind::P2 => Self::P2,
        }
    }
}

/// Offer state row plus the watch kind(s) that matched the query keys.
#[derive(Debug, Clone)]
pub struct WatchHitRow {
    pub row: OfferStateListRow,
    pub kind: WatchMatchKind,
}
