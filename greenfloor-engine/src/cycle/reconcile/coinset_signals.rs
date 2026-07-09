//! Coinset tx signal summary shared by reconcile dispatch paths.
//!
//! WS and Dexie share i64 status codes for dispatch; `DEXIE_STATUS_*` names are historical.

use crate::offer::dexie_payload::{
    DEXIE_STATUS_CANCELLED, DEXIE_STATUS_CONFIRMED, DEXIE_STATUS_EXPIRED,
};

/// Venue-agnostic Coinset tx id lists used by watched-offer reconcile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoinsetTxSignals {
    pub tx_ids: Vec<String>,
    pub confirmed_tx_ids: Vec<String>,
    pub mempool_tx_ids: Vec<String>,
    /// Maker coin/p2 watch hit without a concrete spend-bundle id yet.
    pub watch_hit: bool,
    /// Offer-frame `pending` without a concrete tx id (venue mempool, not a maker watch).
    pub anonymous_mempool: bool,
}

impl CoinsetTxSignals {
    /// Synthetic mempool observation from a durable coin/p2 watch hit.
    #[must_use]
    pub fn watch_hit() -> Self {
        Self {
            watch_hit: true,
            ..Self::default()
        }
    }

    /// Offer WS `pending` with no `tx_id` — mempool activity without fabricating a spend-bundle id.
    #[must_use]
    pub fn offer_pending() -> Self {
        Self {
            anonymous_mempool: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn summary(&self) -> CoinsetSignalSummary {
        CoinsetSignalSummary {
            has_tx_ids: !self.tx_ids.is_empty(),
            has_confirmed: !self.confirmed_tx_ids.is_empty(),
            has_mempool: !self.mempool_tx_ids.is_empty()
                || self.watch_hit
                || self.anonymous_mempool,
            watch_hit: self.watch_hit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // Compact signal flags for reconcile dispatch.
pub struct CoinsetSignalSummary {
    pub has_tx_ids: bool,
    pub has_confirmed: bool,
    pub has_mempool: bool,
    /// True when mempool activity is only a durable coin/p2 watch hit (no concrete tx ids).
    pub watch_hit: bool,
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
            watch_hit: false,
        }
    }

    /// Pure watch hit: no concrete tx ids / confirmations (ignore during `cancel_submitted`).
    #[must_use]
    pub fn is_pure_watch_hit(self) -> bool {
        self.watch_hit && !self.has_tx_ids && !self.has_confirmed
    }

    /// True when any Coinset activity is known (tx ids and/or mempool/confirmed flags).
    #[must_use]
    pub fn has_coinset_activity(self) -> bool {
        self.has_tx_ids || self.has_confirmed || self.has_mempool
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
        // Coinset documents `tx_id` as optional on offer frames. Pending without a
        // concrete tx is still venue mempool activity — not a maker coin/p2 watch hit.
        "pending" => {
            let signals = if tx.is_empty() {
                CoinsetTxSignals::offer_pending()
            } else {
                CoinsetTxSignals {
                    tx_ids: tx.clone(),
                    confirmed_tx_ids: Vec::new(),
                    mempool_tx_ids: tx,
                    ..Default::default()
                }
            };
            Some((None, signals))
        }
        "confirmed" => Some((
            Some(DEXIE_STATUS_CONFIRMED),
            CoinsetTxSignals {
                tx_ids: tx.clone(),
                confirmed_tx_ids: tx,
                mempool_tx_ids: Vec::new(),
                ..Default::default()
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
        assert!(!pending.1.summary().watch_hit);

        let pending_no_tx = signals_from_ws_offer_status("pending", None).expect("pending");
        assert_eq!(pending_no_tx.0, None);
        assert!(pending_no_tx.1.mempool_tx_ids.is_empty());
        assert!(pending_no_tx.1.anonymous_mempool);
        assert!(pending_no_tx.1.summary().has_mempool);
        assert!(!pending_no_tx.1.summary().watch_hit);
        assert!(!pending_no_tx.1.summary().is_pure_watch_hit());

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
    fn watch_hit_summary_is_mempool_without_fabricated_tx_ids() {
        let signals = CoinsetTxSignals::watch_hit();
        let summary = signals.summary();
        assert!(!summary.has_tx_ids);
        assert!(summary.has_mempool);
        assert!(!summary.has_confirmed);
        assert!(summary.watch_hit);
        assert!(summary.is_pure_watch_hit());
        assert!(summary.has_coinset_activity());
        assert!(signals.tx_ids.is_empty());
        assert!(signals.mempool_tx_ids.is_empty());
        assert!(!signals.anonymous_mempool);
        assert!(!CoinsetTxSignals::default().summary().has_mempool);
    }

    #[test]
    fn offer_pending_is_mempool_without_watch_hit() {
        let signals = CoinsetTxSignals::offer_pending();
        let summary = signals.summary();
        assert!(!summary.has_tx_ids);
        assert!(summary.has_mempool);
        assert!(!summary.has_confirmed);
        assert!(!summary.watch_hit);
        assert!(!summary.is_pure_watch_hit());
        assert!(summary.has_coinset_activity());
        assert!(signals.anonymous_mempool);
        assert!(signals.mempool_tx_ids.is_empty());
    }
}
