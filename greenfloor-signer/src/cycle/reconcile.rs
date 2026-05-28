use serde::{Deserialize, Serialize};

use super::lifecycle::{apply_offer_signal, OfferLifecycleState, OfferSignal};

const STATE_CANCELLED: &str = "cancelled";
const STATE_TX_BLOCK_CONFIRMED: &str = "tx_block_confirmed";
const STATE_EXPIRED: &str = "expired";
const TAKER_NONE: &str = "none";

fn is_terminal_reconcile_state(state: &str) -> bool {
    matches!(state, STATE_TX_BLOCK_CONFIRMED | STATE_EXPIRED | STATE_CANCELLED)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleOfferTransition {
    pub old_state: String,
    pub new_state: String,
    pub reason: String,
    pub signal_source: String,
    pub signal: Option<String>,
    pub changed: bool,
    pub immediate_requeue: bool,
    pub taker_signal: String,
    pub taker_diagnostic: String,
    pub coinset_tx_ids: Vec<String>,
    pub coinset_confirmed_tx_ids: Vec<String>,
    pub coinset_mempool_tx_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReconcileDecision {
    new_state: String,
    reason: &'static str,
    signal_source: &'static str,
    signal: Option<OfferSignal>,
    taker_signal: &'static str,
    taker_diagnostic: &'static str,
}

impl ReconcileDecision {
    fn into_transition(
        self,
        current_state: &str,
        coinset_tx_ids: Vec<String>,
        coinset_confirmed_tx_ids: Vec<String>,
        coinset_mempool_tx_ids: Vec<String>,
    ) -> CycleOfferTransition {
        let changed = self.new_state != current_state;
        let signal = if changed {
            self.signal.map(|value| value.as_str().to_string())
        } else {
            None
        };
        let immediate_requeue = changed
            && matches!(
                self.signal,
                Some(OfferSignal::TxConfirmed) | Some(OfferSignal::Expired)
            );
        CycleOfferTransition {
            old_state: current_state.to_string(),
            new_state: self.new_state,
            reason: self.reason.to_string(),
            signal_source: self.signal_source.to_string(),
            signal,
            changed,
            immediate_requeue,
            taker_signal: self.taker_signal.to_string(),
            taker_diagnostic: self.taker_diagnostic.to_string(),
            coinset_tx_ids,
            coinset_confirmed_tx_ids,
            coinset_mempool_tx_ids,
        }
    }
}

fn reconciled_state_from_dexie_status(status: i64, current_state: &str) -> String {
    match status {
        4 => apply_offer_signal(OfferLifecycleState::Open, OfferSignal::TxConfirmed)
            .new_state
            .as_str()
            .to_string(),
        6 => apply_offer_signal(OfferLifecycleState::Open, OfferSignal::Expired)
            .new_state
            .as_str()
            .to_string(),
        3 => STATE_CANCELLED.to_string(),
        0 | 1 | 2 | 5 => current_state.to_string(),
        _ => current_state.to_string(),
    }
}

fn resolve_watched_offer_decision(
    current_state: &str,
    status: Option<i64>,
    coinset_tx_ids: &[String],
    coinset_confirmed_tx_ids: &[String],
    coinset_mempool_tx_ids: &[String],
) -> ReconcileDecision {
    if !coinset_confirmed_tx_ids.is_empty()
        && status != Some(3)
        && current_state != STATE_CANCELLED
    {
        let transition =
            apply_offer_signal(OfferLifecycleState::Open, OfferSignal::TxConfirmed);
        return ReconcileDecision {
            new_state: transition.new_state.as_str().to_string(),
            reason: "coinset_tx_block_webhook_confirmed",
            signal_source: "coinset_webhook",
            signal: Some(OfferSignal::TxConfirmed),
            taker_signal: "coinset_tx_block_webhook",
            taker_diagnostic: "coinset_tx_block_confirmed",
        };
    }

    if !coinset_mempool_tx_ids.is_empty() {
        let new_state = if is_terminal_reconcile_state(current_state) {
            current_state.to_string()
        } else {
            apply_offer_signal(OfferLifecycleState::Open, OfferSignal::MempoolSeen)
                .new_state
                .as_str()
                .to_string()
        };
        let signal = if new_state == OfferLifecycleState::MempoolObserved.as_str() {
            Some(OfferSignal::MempoolSeen)
        } else {
            None
        };
        return ReconcileDecision {
            new_state,
            reason: "coinset_mempool_observed",
            signal_source: "coinset_mempool",
            signal,
            taker_signal: TAKER_NONE,
            taker_diagnostic: "coinset_mempool_observed",
        };
    }

    if status.is_none() {
        if coinset_tx_ids.is_empty() {
            return ReconcileDecision {
                new_state: current_state.to_string(),
                reason: "missing_status",
                signal_source: "none",
                signal: None,
                taker_signal: TAKER_NONE,
                taker_diagnostic: TAKER_NONE,
            };
        }
        return ReconcileDecision {
            new_state: current_state.to_string(),
            reason: "coinset_signal_unavailable_for_offer",
            signal_source: "none",
            signal: None,
            taker_signal: TAKER_NONE,
            taker_diagnostic: TAKER_NONE,
        };
    }

    let dexie_state = reconciled_state_from_dexie_status(status.unwrap_or_default(), current_state);
    let taker_diagnostic = if matches!(status, Some(4) | Some(5)) {
        "dexie_status_pattern_fallback"
    } else {
        TAKER_NONE
    };
    ReconcileDecision {
        new_state: dexie_state.clone(),
        reason: "ok",
        signal_source: "dexie_status_fallback",
        signal: signal_for_state_change(current_state, &dexie_state),
        taker_signal: TAKER_NONE,
        taker_diagnostic,
    }
}

fn signal_for_state_change(current_state: &str, new_state: &str) -> Option<OfferSignal> {
    if new_state == current_state {
        return None;
    }
    if new_state == OfferLifecycleState::TxBlockConfirmed.as_str() {
        return Some(OfferSignal::TxConfirmed);
    }
    if new_state == OfferLifecycleState::Expired.as_str() {
        return Some(OfferSignal::Expired);
    }
    if new_state == OfferLifecycleState::MempoolObserved.as_str() {
        return Some(OfferSignal::MempoolSeen);
    }
    None
}

pub fn unchanged_offer_transition(current_state: &str, reason: impl Into<String>) -> CycleOfferTransition {
    CycleOfferTransition {
        old_state: current_state.to_string(),
        new_state: current_state.to_string(),
        reason: reason.into(),
        signal_source: "none".to_string(),
        signal: None,
        changed: false,
        immediate_requeue: false,
        taker_signal: TAKER_NONE.to_string(),
        taker_diagnostic: TAKER_NONE.to_string(),
        coinset_tx_ids: Vec::new(),
        coinset_confirmed_tx_ids: Vec::new(),
        coinset_mempool_tx_ids: Vec::new(),
    }
}

pub fn unsupported_venue_offer_transition(
    current_state: &str,
    venue: &str,
) -> CycleOfferTransition {
    let new_state = "reconcile_unsupported_venue".to_string();
    CycleOfferTransition {
        old_state: current_state.to_string(),
        new_state: new_state.clone(),
        reason: format!("unsupported_venue:{venue}"),
        signal_source: "none".to_string(),
        signal: None,
        changed: current_state != new_state,
        immediate_requeue: false,
        taker_signal: TAKER_NONE.to_string(),
        taker_diagnostic: TAKER_NONE.to_string(),
        coinset_tx_ids: Vec::new(),
        coinset_confirmed_tx_ids: Vec::new(),
        coinset_mempool_tx_ids: Vec::new(),
    }
}

pub fn resolve_missing_watched_offer_transition(current_state: &str) -> CycleOfferTransition {
    if is_terminal_reconcile_state(current_state) {
        return CycleOfferTransition {
            old_state: current_state.to_string(),
            new_state: current_state.to_string(),
            reason: "dexie_offer_not_found_preserved_terminal".to_string(),
            signal_source: "dexie_get_offer_404".to_string(),
            signal: None,
            changed: false,
            immediate_requeue: false,
            taker_signal: TAKER_NONE.to_string(),
            taker_diagnostic: TAKER_NONE.to_string(),
            coinset_tx_ids: Vec::new(),
            coinset_confirmed_tx_ids: Vec::new(),
            coinset_mempool_tx_ids: Vec::new(),
        };
    }
    let transition = apply_offer_signal(OfferLifecycleState::Open, OfferSignal::Expired);
    CycleOfferTransition {
        old_state: current_state.to_string(),
        new_state: transition.new_state.as_str().to_string(),
        reason: "dexie_offer_not_found".to_string(),
        signal_source: "dexie_get_offer_404".to_string(),
        signal: Some(transition.signal.as_str().to_string()),
        changed: true,
        immediate_requeue: true,
        taker_signal: TAKER_NONE.to_string(),
        taker_diagnostic: TAKER_NONE.to_string(),
        coinset_tx_ids: Vec::new(),
        coinset_confirmed_tx_ids: Vec::new(),
        coinset_mempool_tx_ids: Vec::new(),
    }
}

pub fn resolve_watched_offer_transition_from_signals(
    current_state: &str,
    status: Option<i64>,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
) -> CycleOfferTransition {
    let decision = resolve_watched_offer_decision(
        current_state,
        status,
        &coinset_tx_ids,
        &coinset_confirmed_tx_ids,
        &coinset_mempool_tx_ids,
    );
    decision.into_transition(
        current_state,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coinset_confirmed_moves_open_offer_to_tx_block_confirmed() {
        let transition = resolve_watched_offer_transition_from_signals(
            "open",
            Some(0),
            vec!["c".repeat(64)],
            vec!["c".repeat(64)],
            vec![],
        );
        assert_eq!(transition.new_state, "tx_block_confirmed");
        assert_eq!(transition.reason, "coinset_tx_block_webhook_confirmed");
        assert_eq!(transition.signal_source, "coinset_webhook");
        assert_eq!(transition.signal.as_deref(), Some("tx_confirmed"));
        assert_eq!(transition.taker_signal, "coinset_tx_block_webhook");
        assert_eq!(transition.taker_diagnostic, "coinset_tx_block_confirmed");
    }

    #[test]
    fn coinset_mempool_moves_open_offer_to_mempool_observed() {
        let transition = resolve_watched_offer_transition_from_signals(
            "open",
            Some(0),
            vec!["d".repeat(64)],
            vec![],
            vec!["d".repeat(64)],
        );
        assert_eq!(transition.new_state, "mempool_observed");
        assert_eq!(transition.reason, "coinset_mempool_observed");
        assert_eq!(transition.signal_source, "coinset_mempool");
        assert_eq!(transition.taker_diagnostic, "coinset_mempool_observed");
    }

    #[test]
    fn dexie_fallback_preserves_open_when_no_coinset_signal() {
        let transition = resolve_watched_offer_transition_from_signals(
            "open",
            Some(0),
            vec!["e".repeat(64)],
            vec![],
            vec![],
        );
        assert_eq!(transition.new_state, "open");
        assert_eq!(transition.signal_source, "dexie_status_fallback");
        assert!(!transition.changed);
    }

    #[test]
    fn missing_status_without_tx_ids() {
        let transition =
            resolve_watched_offer_transition_from_signals("open", None, vec![], vec![], vec![]);
        assert_eq!(transition.new_state, "open");
        assert_eq!(transition.reason, "missing_status");
        assert_eq!(transition.signal_source, "none");
    }

    #[test]
    fn dexie_status_fallback_when_no_coinset_tx_ids() {
        let transition =
            resolve_watched_offer_transition_from_signals("open", Some(4), vec![], vec![], vec![]);
        assert_eq!(transition.new_state, "tx_block_confirmed");
        assert_eq!(transition.signal_source, "dexie_status_fallback");
        assert_eq!(transition.taker_diagnostic, "dexie_status_pattern_fallback");
    }

    #[test]
    fn dexie_cancelled_status_fallback() {
        let transition =
            resolve_watched_offer_transition_from_signals("open", Some(3), vec![], vec![], vec![]);
        assert_eq!(transition.new_state, "cancelled");
        assert_eq!(transition.signal_source, "dexie_status_fallback");
    }

    #[test]
    fn missing_watched_offer_expires_open_offer() {
        let transition = resolve_missing_watched_offer_transition("open");
        assert_eq!(transition.new_state, "expired");
        assert!(transition.changed);
        assert!(transition.immediate_requeue);
        assert_eq!(transition.signal.as_deref(), Some("expired"));
    }

    #[test]
    fn missing_watched_offer_preserves_terminal_state() {
        let transition = resolve_missing_watched_offer_transition("tx_block_confirmed");
        assert_eq!(transition.new_state, "tx_block_confirmed");
        assert!(!transition.changed);
    }

    #[test]
    fn unchanged_offer_transition_factory() {
        let transition = unchanged_offer_transition("open", "dexie_lookup_error:boom");
        assert_eq!(transition.old_state, "open");
        assert_eq!(transition.new_state, "open");
        assert!(!transition.changed);
        assert_eq!(transition.taker_signal, "none");
    }

    #[test]
    fn unsupported_venue_offer_transition_factory() {
        let transition = unsupported_venue_offer_transition("open", "splash");
        assert_eq!(transition.new_state, "reconcile_unsupported_venue");
        assert!(transition.changed);
    }
}
