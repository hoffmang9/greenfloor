//! Coinset tx signal summary shared by reconcile dispatch paths.

use crate::offer::dexie_payload::{
    DEXIE_STATUS_CANCELLED, DEXIE_STATUS_CONFIRMED, DEXIE_STATUS_EXPIRED,
};

/// Venue-agnostic Coinset tx id lists used by watched-offer reconcile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoinsetTxSignals {
    pub tx_ids: Vec<String>,
    pub confirmed_tx_ids: Vec<String>,
    pub mempool_tx_ids: Vec<String>,
    /// When true, treat as mempool-observed even with empty tx id lists (watch hit).
    pub(crate) synthetic_mempool: bool,
}

/// Compatibility alias while call sites migrate off the Dexie-named type.
pub type DexieCoinsetSignals = CoinsetTxSignals;

impl CoinsetTxSignals {
    #[must_use]
    pub fn summary(&self) -> CoinsetSignalSummary {
        let mut summary = CoinsetSignalSummary::from_tx_lists(
            &self.tx_ids,
            &self.confirmed_tx_ids,
            &self.mempool_tx_ids,
        );
        if self.synthetic_mempool {
            summary.has_tx_ids = true;
            summary.has_mempool = true;
        }
        summary
    }

    /// Watch-hit / inventory signal with no concrete spend-bundle id yet.
    #[must_use]
    pub fn mempool_hit() -> Self {
        Self {
            synthetic_mempool: true,
            ..Self::default()
        }
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

/// Map a Coinset WS offer status (+ optional tx id) into reconcile status/signals.
///
/// Returns `None` for statuses that only seed `tx_signal_state` (e.g. `cancel_pending`)
/// or are unrecognized.
#[must_use]
pub fn signals_from_ws_offer_status(
    status: &str,
    tx_id: Option<&str>,
) -> Option<(Option<i64>, CoinsetTxSignals)> {
    let tx = tx_id.map(str::to_string).into_iter().collect::<Vec<_>>();
    match status {
        "pending" => Some((
            None,
            CoinsetTxSignals {
                tx_ids: tx.clone(),
                confirmed_tx_ids: Vec::new(),
                mempool_tx_ids: tx,
                ..CoinsetTxSignals::default()
            },
        )),
        "confirmed" => Some((
            Some(DEXIE_STATUS_CONFIRMED),
            CoinsetTxSignals {
                tx_ids: tx.clone(),
                confirmed_tx_ids: tx,
                mempool_tx_ids: Vec::new(),
                ..CoinsetTxSignals::default()
            },
        )),
        "expired" => Some((Some(DEXIE_STATUS_EXPIRED), CoinsetTxSignals::default())),
        "cancelled" => Some((Some(DEXIE_STATUS_CANCELLED), CoinsetTxSignals::default())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_from_ws_offer_status_matrix() {
        let tx = "ab".repeat(32);
        let pending = signals_from_ws_offer_status("pending", Some(&tx)).expect("pending");
        assert_eq!(pending.0, None);
        assert_eq!(pending.1.mempool_tx_ids, vec![tx.clone()]);
        assert!(pending.1.summary().has_mempool);

        let confirmed = signals_from_ws_offer_status("confirmed", Some(&tx)).expect("confirmed");
        assert_eq!(confirmed.0, Some(DEXIE_STATUS_CONFIRMED));
        assert_eq!(confirmed.1.confirmed_tx_ids, vec![tx]);
        assert!(confirmed.1.summary().has_confirmed);
        assert!(!confirmed.1.summary().has_mempool);

        assert_eq!(
            signals_from_ws_offer_status("expired", None)
                .expect("expired")
                .0,
            Some(DEXIE_STATUS_EXPIRED)
        );
        assert_eq!(
            signals_from_ws_offer_status("cancelled", None)
                .expect("cancelled")
                .0,
            Some(DEXIE_STATUS_CANCELLED)
        );
        assert!(
            signals_from_ws_offer_status("cancel_pending", Some("cd".repeat(32).as_str()))
                .is_none()
        );
        assert!(signals_from_ws_offer_status("unknown", None).is_none());
    }

    #[test]
    fn mempool_hit_summary_is_synthetic_without_tx_ids() {
        let hit = CoinsetTxSignals::mempool_hit();
        assert!(hit.tx_ids.is_empty());
        assert!(hit.mempool_tx_ids.is_empty());
        let summary = hit.summary();
        assert!(summary.has_tx_ids);
        assert!(summary.has_mempool);
        assert!(!summary.has_confirmed);
        assert_eq!(summary, CoinsetSignalSummary::mempool_hit());
    }
}
