use serde::{Deserialize, Serialize};

use super::lifecycle::{apply_offer_signal, OfferLifecycleState, OfferSignal};

const STATE_CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleOfferTransition {
    pub old_state: String,
    pub new_state: String,
    pub reason: String,
    pub signal_source: String,
    pub signal: Option<String>,
    pub changed: bool,
    pub immediate_requeue: bool,
    pub coinset_tx_ids: Vec<String>,
    pub coinset_confirmed_tx_ids: Vec<String>,
    pub coinset_mempool_tx_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakerFields {
    pub taker_signal: String,
    pub taker_diagnostic: String,
}

pub fn reconciled_state_from_dexie_status(status: i64, current_state: &str) -> String {
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

fn apply_coinset_signals(
    current_state: &str,
    status: Option<i64>,
    coinset_confirmed_tx_ids: &[String],
    coinset_mempool_tx_ids: &[String],
) -> (String, String, String) {
    if !coinset_confirmed_tx_ids.is_empty()
        && status != Some(3)
        && current_state != STATE_CANCELLED
    {
        let transition =
            apply_offer_signal(OfferLifecycleState::Open, OfferSignal::TxConfirmed);
        return (
            transition.new_state.as_str().to_string(),
            "coinset_tx_block_webhook_confirmed".to_string(),
            "coinset_webhook".to_string(),
        );
    }
    if !coinset_mempool_tx_ids.is_empty() {
        let next_state = if matches!(
            current_state,
            "tx_block_confirmed" | "expired" | STATE_CANCELLED
        ) {
            current_state.to_string()
        } else {
            apply_offer_signal(OfferLifecycleState::Open, OfferSignal::MempoolSeen)
                .new_state
                .as_str()
                .to_string()
        };
        return (
            next_state,
            "coinset_mempool_observed".to_string(),
            "coinset_mempool".to_string(),
        );
    }
    (
        current_state.to_string(),
        "ok".to_string(),
        "none".to_string(),
    )
}

fn apply_dexie_status_fallback(
    status: Option<i64>,
    current_state: &str,
    coinset_tx_ids: &[String],
    signal_source: &str,
    next_state: &str,
    reason: &str,
) -> (String, String, String) {
    if status.is_none() {
        if coinset_tx_ids.is_empty() {
            return (
                current_state.to_string(),
                "missing_status".to_string(),
                signal_source.to_string(),
            );
        }
        if signal_source == "none" {
            return (
                current_state.to_string(),
                "coinset_signal_unavailable_for_offer".to_string(),
                signal_source.to_string(),
            );
        }
        return (
            next_state.to_string(),
            reason.to_string(),
            signal_source.to_string(),
        );
    }
    if signal_source == "none" {
        return (
            reconciled_state_from_dexie_status(status.unwrap_or_default(), current_state),
            reason.to_string(),
            "dexie_status_fallback".to_string(),
        );
    }
    (
        next_state.to_string(),
        reason.to_string(),
        signal_source.to_string(),
    )
}

pub fn taker_fields(
    coinset_confirmed_tx_ids: &[String],
    coinset_mempool_tx_ids: &[String],
    status: Option<i64>,
    current_state: &str,
    next_state: &str,
) -> TakerFields {
    if !coinset_confirmed_tx_ids.is_empty()
        && status != Some(3)
        && current_state != STATE_CANCELLED
        && next_state == OfferLifecycleState::TxBlockConfirmed.as_str()
    {
        return TakerFields {
            taker_signal: "coinset_tx_block_webhook".to_string(),
            taker_diagnostic: "coinset_tx_block_confirmed".to_string(),
        };
    }
    if !coinset_mempool_tx_ids.is_empty() {
        return TakerFields {
            taker_signal: "none".to_string(),
            taker_diagnostic: "coinset_mempool_observed".to_string(),
        };
    }
    if matches!(status, Some(4) | Some(5)) {
        return TakerFields {
            taker_signal: "none".to_string(),
            taker_diagnostic: "dexie_status_pattern_fallback".to_string(),
        };
    }
    TakerFields {
        taker_signal: "none".to_string(),
        taker_diagnostic: "none".to_string(),
    }
}

impl CycleOfferTransition {
    pub fn taker_fields(&self, last_seen_status: Option<i64>) -> TakerFields {
        taker_fields(
            &self.coinset_confirmed_tx_ids,
            &self.coinset_mempool_tx_ids,
            last_seen_status,
            &self.old_state,
            &self.new_state,
        )
    }
}

pub fn resolve_missing_watched_offer_transition(current_state: &str) -> CycleOfferTransition {
    if matches!(
        current_state,
        "tx_block_confirmed" | "expired" | STATE_CANCELLED
    ) {
        return CycleOfferTransition {
            old_state: current_state.to_string(),
            new_state: current_state.to_string(),
            reason: "dexie_offer_not_found_preserved_terminal".to_string(),
            signal_source: "dexie_get_offer_404".to_string(),
            signal: None,
            changed: false,
            immediate_requeue: false,
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
        coinset_tx_ids: Vec::new(),
        coinset_confirmed_tx_ids: Vec::new(),
        coinset_mempool_tx_ids: Vec::new(),
    }
}

pub fn resolve_watched_offer_transition(
    current_state: &str,
    status: Option<i64>,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
) -> CycleOfferTransition {
    let (mut next_state, mut reason, mut signal_source) = apply_coinset_signals(
        current_state,
        status,
        &coinset_confirmed_tx_ids,
        &coinset_mempool_tx_ids,
    );
    (next_state, reason, signal_source) = apply_dexie_status_fallback(
        status,
        current_state,
        &coinset_tx_ids,
        &signal_source,
        &next_state,
        &reason,
    );
    let changed = next_state != current_state;
    let signal = if changed {
        if next_state == OfferLifecycleState::TxBlockConfirmed.as_str() {
            Some(OfferSignal::TxConfirmed.as_str().to_string())
        } else if next_state == OfferLifecycleState::Expired.as_str() {
            Some(OfferSignal::Expired.as_str().to_string())
        } else if next_state == OfferLifecycleState::MempoolObserved.as_str() {
            Some(OfferSignal::MempoolSeen.as_str().to_string())
        } else {
            None
        }
    } else {
        None
    };
    let immediate_requeue = changed
        && matches!(
            signal.as_deref(),
            Some("tx_confirmed") | Some("expired")
        );
    CycleOfferTransition {
        old_state: current_state.to_string(),
        new_state: next_state,
        reason,
        signal_source,
        signal,
        changed,
        immediate_requeue,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coinset_confirmed_moves_open_offer_to_tx_block_confirmed() {
        let transition = resolve_watched_offer_transition(
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
        let taker = transition.taker_fields(Some(0));
        assert_eq!(taker.taker_signal, "coinset_tx_block_webhook");
        assert_eq!(taker.taker_diagnostic, "coinset_tx_block_confirmed");
    }

    #[test]
    fn coinset_mempool_moves_open_offer_to_mempool_observed() {
        let transition = resolve_watched_offer_transition(
            "open",
            Some(0),
            vec!["d".repeat(64)],
            vec![],
            vec!["d".repeat(64)],
        );
        assert_eq!(transition.new_state, "mempool_observed");
        assert_eq!(transition.reason, "coinset_mempool_observed");
        assert_eq!(transition.signal_source, "coinset_mempool");
        let taker = transition.taker_fields(Some(0));
        assert_eq!(taker.taker_diagnostic, "coinset_mempool_observed");
    }

    #[test]
    fn dexie_fallback_preserves_open_when_no_coinset_signal() {
        let transition = resolve_watched_offer_transition(
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
        let transition = resolve_watched_offer_transition("open", None, vec![], vec![], vec![]);
        assert_eq!(transition.new_state, "open");
        assert_eq!(transition.reason, "missing_status");
        assert_eq!(transition.signal_source, "none");
    }

    #[test]
    fn dexie_status_fallback_when_no_coinset_tx_ids() {
        let transition = resolve_watched_offer_transition("open", Some(4), vec![], vec![], vec![]);
        assert_eq!(transition.new_state, "tx_block_confirmed");
        assert_eq!(transition.signal_source, "dexie_status_fallback");
        let taker = transition.taker_fields(Some(4));
        assert_eq!(taker.taker_diagnostic, "dexie_status_pattern_fallback");
    }

    #[test]
    fn dexie_cancelled_status_fallback() {
        let transition = resolve_watched_offer_transition("open", Some(3), vec![], vec![], vec![]);
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
}
