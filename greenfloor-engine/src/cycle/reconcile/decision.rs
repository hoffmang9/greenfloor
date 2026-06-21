use crate::cycle::lifecycle::{apply_offer_signal, OfferLifecycleState, OfferSignal};

use super::state::{ReconcileState, TAKER_NONE};
use super::transition::ReconcileDecision;

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

pub(crate) fn resolve_watched_offer_decision(
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
