//! Shared Dexie reconcile transition apply + cycle metrics.

use std::collections::HashMap;

use crate::cycle::CycleOfferTransition;

#[derive(Debug, Clone, Default)]
pub struct ReconcileMarketCycleMetrics {
    pub cycle_errors: u64,
    pub immediate_requeue_requested: bool,
    pub immediate_requeue_signals: Vec<String>,
}

/// Update cycle metrics / in-memory state after a lifecycle transition was applied.
pub(crate) fn note_reconcile_transition_side_effects(
    transition: &CycleOfferTransition,
    offer_id: &str,
    metrics: &mut ReconcileMarketCycleMetrics,
    state_by_offer_id: &mut HashMap<String, String>,
) {
    if transition.changed {
        state_by_offer_id.insert(
            offer_id.to_string(),
            transition.new_state.as_str().into_owned(),
        );
    }
    if transition.immediate_requeue {
        metrics.immediate_requeue_requested = true;
        if let Some(signal) = transition.signal {
            metrics
                .immediate_requeue_signals
                .push(signal.as_str().to_string());
        }
    }
}

pub fn merge_reconcile_immediate_requeue(
    state: &mut crate::cycle::MarketCycleResultState,
    metrics: &ReconcileMarketCycleMetrics,
) {
    if !metrics.immediate_requeue_requested {
        return;
    }
    for signal in &metrics.immediate_requeue_signals {
        state.request_immediate_requeue(Some(signal.clone()));
    }
    if metrics.immediate_requeue_signals.is_empty() {
        state.request_immediate_requeue(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::MarketCycleResultState;

    #[test]
    fn merge_reconcile_immediate_requeue_populates_cycle_state() {
        let mut state = MarketCycleResultState::default();
        let metrics = ReconcileMarketCycleMetrics {
            immediate_requeue_requested: true,
            immediate_requeue_signals: vec!["taker_fill".to_string()],
            ..ReconcileMarketCycleMetrics::default()
        };
        merge_reconcile_immediate_requeue(&mut state, &metrics);
        assert!(state.immediate_requeue_requested);
        assert_eq!(
            state.immediate_requeue_signals,
            vec!["taker_fill".to_string()]
        );
    }

    #[test]
    fn merge_reconcile_immediate_requeue_without_signal_still_flags() {
        let mut state = MarketCycleResultState::default();
        let metrics = ReconcileMarketCycleMetrics {
            immediate_requeue_requested: true,
            ..ReconcileMarketCycleMetrics::default()
        };
        merge_reconcile_immediate_requeue(&mut state, &metrics);
        assert!(state.immediate_requeue_requested);
        assert!(state.immediate_requeue_signals.is_empty());
    }
}
