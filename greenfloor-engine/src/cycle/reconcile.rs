//! Offer transition policy for daemon cycle signals (taker detection, venue rules).
//!
//! Batch DB reconcile lives in `offer::lifecycle::reconcile_watched_offers`; per-market
//! cycle reconcile lives in `daemon::reconcile_market_cycle`.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use super::lifecycle::{apply_offer_signal, OfferLifecycleState, OfferSignal};

const TAKER_NONE: &str = "none";
const STATE_UNSUPPORTED_VENUE: &str = "reconcile_unsupported_venue";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileStateError {
    state: String,
}

impl std::fmt::Display for ReconcileStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown offer reconcile state: {}", self.state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReconcileState {
    Lifecycle(OfferLifecycleState),
    Cancelled,
    UnsupportedVenue,
}

impl ReconcileState {
    fn parse(raw: &str) -> Result<Self, ReconcileStateError> {
        match raw.trim() {
            "open" => Ok(Self::Lifecycle(OfferLifecycleState::Open)),
            "mempool_observed" => Ok(Self::Lifecycle(OfferLifecycleState::MempoolObserved)),
            "tx_block_confirmed" => Ok(Self::Lifecycle(OfferLifecycleState::TxBlockConfirmed)),
            "refresh_due" => Ok(Self::Lifecycle(OfferLifecycleState::RefreshDue)),
            "expired" => Ok(Self::Lifecycle(OfferLifecycleState::Expired)),
            "cancelled" => Ok(Self::Cancelled),
            STATE_UNSUPPORTED_VENUE => Ok(Self::UnsupportedVenue),
            other => Err(ReconcileStateError {
                state: other.to_string(),
            }),
        }
    }

    fn as_str(&self) -> Cow<'_, str> {
        match self {
            Self::Lifecycle(state) => Cow::Borrowed(state.as_str()),
            Self::Cancelled => Cow::Borrowed("cancelled"),
            Self::UnsupportedVenue => Cow::Borrowed(STATE_UNSUPPORTED_VENUE),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Lifecycle(OfferLifecycleState::TxBlockConfirmed | OfferLifecycleState::Expired)
                | Self::Cancelled
        )
    }

    fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
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
    new_state: ReconcileState,
    reason: &'static str,
    signal_source: &'static str,
    signal: Option<OfferSignal>,
    taker_signal: &'static str,
    taker_diagnostic: &'static str,
}

struct TransitionBuildArgs<'a> {
    current_state: &'a str,
    new_state: ReconcileState,
    reason: Cow<'a, str>,
    signal_source: &'static str,
    signal: Option<OfferSignal>,
    immediate_requeue: bool,
    taker_signal: &'static str,
    taker_diagnostic: &'static str,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
}

fn empty_coinset_lists() -> (Vec<String>, Vec<String>, Vec<String>) {
    (Vec::new(), Vec::new(), Vec::new())
}

fn build_transition(args: TransitionBuildArgs<'_>) -> CycleOfferTransition {
    let new_state = args.new_state.as_str().into_owned();
    let changed = new_state != args.current_state;
    let signal = if changed {
        args.signal.map(|value| value.as_str().to_string())
    } else {
        None
    };
    CycleOfferTransition {
        old_state: args.current_state.to_string(),
        new_state,
        reason: args.reason.into_owned(),
        signal_source: args.signal_source.to_string(),
        signal,
        changed,
        immediate_requeue: args.immediate_requeue && changed,
        taker_signal: args.taker_signal.to_string(),
        taker_diagnostic: args.taker_diagnostic.to_string(),
        coinset_tx_ids: args.coinset_tx_ids,
        coinset_confirmed_tx_ids: args.coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids: args.coinset_mempool_tx_ids,
    }
}

fn build_transition_from_decision(
    current_state: &str,
    decision: ReconcileDecision,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
) -> CycleOfferTransition {
    let immediate_requeue = matches!(
        decision.signal,
        Some(OfferSignal::TxConfirmed | OfferSignal::Expired)
    );
    build_transition(TransitionBuildArgs {
        current_state,
        new_state: decision.new_state,
        reason: Cow::Borrowed(decision.reason),
        signal_source: decision.signal_source,
        signal: decision.signal,
        immediate_requeue,
        taker_signal: decision.taker_signal,
        taker_diagnostic: decision.taker_diagnostic,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    })
}

fn reconciled_state_from_dexie_status(
    status: i64,
    current_state: &ReconcileState,
) -> ReconcileState {
    match status {
        4 => ReconcileState::Lifecycle(
            apply_offer_signal(OfferLifecycleState::Open, OfferSignal::TxConfirmed).new_state,
        ),
        6 => ReconcileState::Lifecycle(
            apply_offer_signal(OfferLifecycleState::Open, OfferSignal::Expired).new_state,
        ),
        3 => ReconcileState::Cancelled,
        _ => current_state.clone(),
    }
}

fn resolve_watched_offer_decision(
    current_state: &ReconcileState,
    status: Option<i64>,
    coinset_tx_ids: &[String],
    coinset_confirmed_tx_ids: &[String],
    coinset_mempool_tx_ids: &[String],
) -> ReconcileDecision {
    // Coinset confirmed/mempool paths apply lifecycle signals from `Open`, not from
    // `current_state`. Terminal-state guard on mempool preserves completed offers.
    if !coinset_confirmed_tx_ids.is_empty() && status != Some(3) && !current_state.is_cancelled() {
        let transition = apply_offer_signal(OfferLifecycleState::Open, OfferSignal::TxConfirmed);
        return ReconcileDecision {
            new_state: ReconcileState::Lifecycle(transition.new_state),
            reason: "coinset_tx_block_webhook_confirmed",
            signal_source: "coinset_webhook",
            signal: Some(OfferSignal::TxConfirmed),
            taker_signal: "coinset_tx_block_webhook",
            taker_diagnostic: "coinset_tx_block_confirmed",
        };
    }

    if !coinset_mempool_tx_ids.is_empty() {
        let new_state = if current_state.is_terminal() {
            current_state.clone()
        } else {
            ReconcileState::Lifecycle(
                apply_offer_signal(OfferLifecycleState::Open, OfferSignal::MempoolSeen).new_state,
            )
        };
        let signal = match &new_state {
            ReconcileState::Lifecycle(OfferLifecycleState::MempoolObserved) => {
                Some(OfferSignal::MempoolSeen)
            }
            _ => None,
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
                new_state: current_state.clone(),
                reason: "missing_status",
                signal_source: "none",
                signal: None,
                taker_signal: TAKER_NONE,
                taker_diagnostic: TAKER_NONE,
            };
        }
        return ReconcileDecision {
            new_state: current_state.clone(),
            reason: "coinset_signal_unavailable_for_offer",
            signal_source: "none",
            signal: None,
            taker_signal: TAKER_NONE,
            taker_diagnostic: TAKER_NONE,
        };
    }

    let dexie_state = reconciled_state_from_dexie_status(status.unwrap_or_default(), current_state);
    let taker_diagnostic = if matches!(status, Some(4 | 5)) {
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

fn signal_for_state_change(
    current_state: &ReconcileState,
    new_state: &ReconcileState,
) -> Option<OfferSignal> {
    if new_state == current_state {
        return None;
    }
    match new_state {
        ReconcileState::Lifecycle(OfferLifecycleState::TxBlockConfirmed) => {
            Some(OfferSignal::TxConfirmed)
        }
        ReconcileState::Lifecycle(OfferLifecycleState::Expired) => Some(OfferSignal::Expired),
        ReconcileState::Lifecycle(OfferLifecycleState::MempoolObserved) => {
            Some(OfferSignal::MempoolSeen)
        }
        _ => None,
    }
}

pub fn unchanged_offer_transition(
    current_state: &str,
    reason: impl Into<String>,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) = empty_coinset_lists();
    Ok(build_transition(TransitionBuildArgs {
        current_state,
        new_state: ReconcileState::parse(current_state)?,
        reason: Cow::Owned(reason.into()),
        signal_source: "none",
        signal: None,
        immediate_requeue: false,
        taker_signal: TAKER_NONE,
        taker_diagnostic: TAKER_NONE,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    }))
}

pub fn unsupported_venue_offer_transition(
    current_state: &str,
    venue: &str,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    ReconcileState::parse(current_state)?;
    let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) = empty_coinset_lists();
    Ok(build_transition(TransitionBuildArgs {
        current_state,
        new_state: ReconcileState::UnsupportedVenue,
        reason: Cow::Owned(format!("unsupported_venue:{venue}")),
        signal_source: "none",
        signal: None,
        immediate_requeue: false,
        taker_signal: TAKER_NONE,
        taker_diagnostic: TAKER_NONE,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    }))
}

pub fn resolve_missing_watched_offer_transition(
    current_state: &str,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let parsed = ReconcileState::parse(current_state)?;
    let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) = empty_coinset_lists();
    if parsed.is_terminal() {
        return Ok(build_transition(TransitionBuildArgs {
            current_state,
            new_state: parsed,
            reason: Cow::Borrowed("dexie_offer_not_found_preserved_terminal"),
            signal_source: "dexie_get_offer_404",
            signal: None,
            immediate_requeue: false,
            taker_signal: TAKER_NONE,
            taker_diagnostic: TAKER_NONE,
            coinset_tx_ids,
            coinset_confirmed_tx_ids,
            coinset_mempool_tx_ids,
        }));
    }
    let transition = apply_offer_signal(OfferLifecycleState::Open, OfferSignal::Expired);
    Ok(build_transition(TransitionBuildArgs {
        current_state,
        new_state: ReconcileState::Lifecycle(transition.new_state),
        reason: Cow::Borrowed("dexie_offer_not_found"),
        signal_source: "dexie_get_offer_404",
        signal: Some(OfferSignal::Expired),
        immediate_requeue: true,
        taker_signal: TAKER_NONE,
        taker_diagnostic: TAKER_NONE,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    }))
}

pub fn resolve_watched_offer_transition_from_signals(
    current_state: &str,
    status: Option<i64>,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let parsed = ReconcileState::parse(current_state)?;
    let decision = resolve_watched_offer_decision(
        &parsed,
        status,
        &coinset_tx_ids,
        &coinset_confirmed_tx_ids,
        &coinset_mempool_tx_ids,
    );
    Ok(build_transition_from_decision(
        current_state,
        decision,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    ))
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
        )
        .expect("valid reconcile state");
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
        )
        .expect("valid reconcile state");
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
        )
        .expect("valid reconcile state");
        assert_eq!(transition.new_state, "open");
        assert_eq!(transition.signal_source, "dexie_status_fallback");
        assert!(!transition.changed);
    }

    #[test]
    fn missing_status_without_tx_ids() {
        let transition =
            resolve_watched_offer_transition_from_signals("open", None, vec![], vec![], vec![])
                .expect("valid reconcile state");
        assert_eq!(transition.new_state, "open");
        assert_eq!(transition.reason, "missing_status");
        assert_eq!(transition.signal_source, "none");
    }

    #[test]
    fn coinset_signal_unavailable_for_offer() {
        let transition = resolve_watched_offer_transition_from_signals(
            "open",
            None,
            vec!["f".repeat(64)],
            vec![],
            vec![],
        )
        .expect("valid reconcile state");
        assert_eq!(transition.new_state, "open");
        assert_eq!(transition.reason, "coinset_signal_unavailable_for_offer");
        assert_eq!(transition.signal_source, "none");
    }

    #[test]
    fn dexie_status_fallback_when_no_coinset_tx_ids() {
        let transition =
            resolve_watched_offer_transition_from_signals("open", Some(4), vec![], vec![], vec![])
                .expect("valid reconcile state");
        assert_eq!(transition.new_state, "tx_block_confirmed");
        assert_eq!(transition.signal_source, "dexie_status_fallback");
        assert_eq!(transition.taker_diagnostic, "dexie_status_pattern_fallback");
    }

    #[test]
    fn dexie_cancelled_status_fallback() {
        let transition =
            resolve_watched_offer_transition_from_signals("open", Some(3), vec![], vec![], vec![])
                .expect("valid reconcile state");
        assert_eq!(transition.new_state, "cancelled");
        assert_eq!(transition.signal_source, "dexie_status_fallback");
    }

    #[test]
    fn missing_watched_offer_expires_open_offer() {
        let transition =
            resolve_missing_watched_offer_transition("open").expect("valid reconcile state");
        assert_eq!(transition.new_state, "expired");
        assert!(transition.changed);
        assert!(transition.immediate_requeue);
        assert_eq!(transition.signal.as_deref(), Some("expired"));
    }

    #[test]
    fn missing_watched_offer_preserves_terminal_state() {
        let transition = resolve_missing_watched_offer_transition("tx_block_confirmed")
            .expect("valid reconcile state");
        assert_eq!(transition.new_state, "tx_block_confirmed");
        assert!(!transition.changed);
    }

    #[test]
    fn unchanged_offer_transition_factory() {
        let transition = unchanged_offer_transition("open", "dexie_lookup_error:boom")
            .expect("valid reconcile state");
        assert_eq!(transition.old_state, "open");
        assert_eq!(transition.new_state, "open");
        assert!(!transition.changed);
        assert_eq!(transition.taker_signal, "none");
    }

    #[test]
    fn unsupported_venue_offer_transition_factory() {
        let transition =
            unsupported_venue_offer_transition("open", "splash").expect("valid reconcile state");
        assert_eq!(transition.new_state, "reconcile_unsupported_venue");
        assert!(transition.changed);
    }

    #[test]
    fn unknown_reconcile_state_is_rejected() {
        let err = resolve_watched_offer_transition_from_signals(
            "not_a_real_state",
            None,
            vec![],
            vec![],
            vec![],
        )
        .expect_err("unknown state should fail");
        assert_eq!(
            err.to_string(),
            "unknown offer reconcile state: not_a_real_state"
        );
    }
}
