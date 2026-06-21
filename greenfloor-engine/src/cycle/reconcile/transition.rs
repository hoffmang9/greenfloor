use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::cycle::lifecycle::OfferSignal;

use super::state::ReconcileState;

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
pub(crate) struct ReconcileTransition {
    new_state: ReconcileState,
    reason: Cow<'static, str>,
    signal_source: &'static str,
    signal: Option<OfferSignal>,
    taker_signal: &'static str,
    taker_diagnostic: &'static str,
}

impl ReconcileTransition {
    pub(crate) fn new(
        new_state: ReconcileState,
        reason: &'static str,
        signal_source: &'static str,
        signal: Option<OfferSignal>,
        taker_signal: &'static str,
        taker_diagnostic: &'static str,
    ) -> Self {
        Self {
            new_state,
            reason: Cow::Borrowed(reason),
            signal_source,
            signal,
            taker_signal,
            taker_diagnostic,
        }
    }

    pub(crate) fn with_owned_reason(
        new_state: ReconcileState,
        reason: Cow<'static, str>,
        signal_source: &'static str,
        signal: Option<OfferSignal>,
        taker_signal: &'static str,
        taker_diagnostic: &'static str,
    ) -> Self {
        Self {
            new_state,
            reason,
            signal_source,
            signal,
            taker_signal,
            taker_diagnostic,
        }
    }

    fn immediate_requeue(&self, changed: bool) -> bool {
        changed
            && matches!(
                self.signal,
                Some(OfferSignal::TxConfirmed | OfferSignal::Expired)
            )
    }

    pub(crate) fn into_cycle_transition(
        self,
        current_state: &str,
        coinset_tx_ids: Vec<String>,
        coinset_confirmed_tx_ids: Vec<String>,
        coinset_mempool_tx_ids: Vec<String>,
    ) -> CycleOfferTransition {
        let new_state = self.new_state.as_str().into_owned();
        let changed = new_state != current_state;
        let signal = if changed {
            self.signal.map(|value| value.as_str().to_string())
        } else {
            None
        };
        let immediate_requeue = self.immediate_requeue(changed);
        CycleOfferTransition {
            old_state: current_state.to_string(),
            new_state,
            reason: self.reason.into_owned(),
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

    pub(crate) fn into_cycle_transition_no_coinset(
        self,
        current_state: &str,
    ) -> CycleOfferTransition {
        self.into_cycle_transition(current_state, Vec::new(), Vec::new(), Vec::new())
    }
}
