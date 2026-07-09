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

    /// Drop the tracked cancel spend-bundle id so it cannot look like taker activity.
    #[must_use]
    pub fn excluding_cancel_tx(&self, cancel_tx_id: Option<&str>) -> Self {
        let Some(cancel) = cancel_tx_id.and_then(crate::hex::canonical_tx_id) else {
            return self.clone();
        };
        let drop_cancel = |ids: &[String]| -> Vec<String> {
            ids.iter()
                .filter(|id| crate::hex::canonical_tx_id(id).as_deref() != Some(cancel.as_str()))
                .cloned()
                .collect()
        };
        Self {
            tx_ids: drop_cancel(&self.tx_ids),
            confirmed_tx_ids: drop_cancel(&self.confirmed_tx_ids),
            mempool_tx_ids: drop_cancel(&self.mempool_tx_ids),
            watch_hit: self.watch_hit,
        }
    }

    #[must_use]
    pub fn summary(&self) -> CoinsetSignalSummary {
        CoinsetSignalSummary {
            has_tx_ids: !self.tx_ids.is_empty(),
            has_confirmed: !self.confirmed_tx_ids.is_empty(),
            has_mempool: !self.mempool_tx_ids.is_empty() || self.watch_hit,
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

/// Why a Coinset signal must not leave `cancel_submitted` via taker dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelSubmitNonAttributable {
    WatchHit,
    CancelTxMempool,
}

/// Strip tracked cancel spend and classify noise that must preserve `cancel_submitted`.
///
/// `Ok(summary)` is safe for taker dispatch; `Err(kind)` keeps cancel-tx promotion eligible.
pub(crate) fn cancel_submit_taker_signals(
    signals: &CoinsetTxSignals,
    cancel_tx_id: Option<&str>,
) -> Result<CoinsetSignalSummary, CancelSubmitNonAttributable> {
    let original = signals.summary();
    if original.is_pure_watch_hit() {
        return Err(CancelSubmitNonAttributable::WatchHit);
    }
    let stripped = signals.excluding_cancel_tx(cancel_tx_id).summary();
    // Tracked cancel spend in mempool/tx lists only — strip left nothing for taker.
    if original.has_coinset_activity()
        && !original.has_confirmed
        && !stripped.has_coinset_activity()
    {
        return Err(CancelSubmitNonAttributable::CancelTxMempool);
    }
    Ok(stripped)
}

/// Map a Coinset WS offer status (+ optional tx id) into reconcile status/signals.
///
/// Returns `None` for statuses that only seed `tx_signal_state` (`pending`,
/// `cancel_pending`) or are unrecognized. Offer-frame `pending` must not drive
/// `mempool_observed`: that state ages out of active-slot counts after ~3 minutes
/// while the listing can still be live on Coinset, which would allow duplicate
/// ladder posts. Take detection stays on durable maker watch hits and `confirmed`
/// / terminal offer statuses.
#[must_use]
pub fn signals_from_ws_offer_status(
    status: &str,
    tx_id: Option<&str>,
) -> Option<(Option<i64>, CoinsetTxSignals)> {
    let tx = tx_id.map(str::to_string).into_iter().collect::<Vec<_>>();
    match status {
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
        // `pending` / `cancel_pending`: seed tx via apply path only (see ws_apply).
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_from_ws_offer_status_matrix() {
        let tx = "ab".repeat(32);
        // Pending is seed-only (tx_signal_state); must not invent mempool lifecycle.
        assert!(signals_from_ws_offer_status("pending", Some(&tx)).is_none());
        assert!(signals_from_ws_offer_status("pending", None).is_none());

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
        assert!(!CoinsetTxSignals::default().summary().has_mempool);
    }

    #[test]
    fn excluding_cancel_tx_drops_tracked_id_from_all_lists() {
        let cancel = "aa".repeat(32);
        let taker = "bb".repeat(32);
        let signals = CoinsetTxSignals {
            tx_ids: vec![cancel.clone(), taker.clone()],
            confirmed_tx_ids: vec![cancel.clone()],
            mempool_tx_ids: vec![cancel.clone(), taker.clone()],
            ..Default::default()
        };
        let stripped = signals.excluding_cancel_tx(Some(&cancel));
        assert_eq!(stripped.tx_ids, vec![taker.clone()]);
        assert!(stripped.confirmed_tx_ids.is_empty());
        assert_eq!(stripped.mempool_tx_ids, vec![taker]);
    }

    #[test]
    fn cancel_submit_taker_signals_classifies_non_attributable_noise() {
        assert_eq!(
            cancel_submit_taker_signals(&CoinsetTxSignals::watch_hit(), None),
            Err(CancelSubmitNonAttributable::WatchHit)
        );
        let cancel = "aa".repeat(32);
        let cancel_only = CoinsetTxSignals {
            tx_ids: vec![cancel.clone()],
            mempool_tx_ids: vec![cancel.clone()],
            ..Default::default()
        };
        assert_eq!(
            cancel_submit_taker_signals(&cancel_only, Some(&cancel)),
            Err(CancelSubmitNonAttributable::CancelTxMempool)
        );
        let taker = "bb".repeat(32);
        let cancel_id = "aa".repeat(32);
        let with_taker = CoinsetTxSignals {
            tx_ids: vec![cancel_id.clone(), taker.clone()],
            mempool_tx_ids: vec![cancel_id.clone(), taker.clone()],
            ..Default::default()
        };
        let summary =
            cancel_submit_taker_signals(&with_taker, Some(&cancel_id)).expect("taker remains");
        assert!(summary.has_mempool);
        assert!(summary.has_tx_ids);
        assert!(!summary.has_confirmed);
    }
}
