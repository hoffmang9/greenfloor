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
pub(crate) struct ReconcileDecision {
    pub(crate) new_state: ReconcileState,
    pub(crate) reason: &'static str,
    pub(crate) signal_source: &'static str,
    pub(crate) signal: Option<OfferSignal>,
    pub(crate) taker_signal: &'static str,
    pub(crate) taker_diagnostic: &'static str,
}

pub(crate) struct TransitionBuildArgs<'a> {
    pub(crate) current_state: &'a str,
    pub(crate) new_state: ReconcileState,
    pub(crate) reason: Cow<'a, str>,
    pub(crate) signal_source: &'static str,
    pub(crate) signal: Option<OfferSignal>,
    pub(crate) immediate_requeue: bool,
    pub(crate) taker_signal: &'static str,
    pub(crate) taker_diagnostic: &'static str,
    pub(crate) coinset_tx_ids: Vec<String>,
    pub(crate) coinset_confirmed_tx_ids: Vec<String>,
    pub(crate) coinset_mempool_tx_ids: Vec<String>,
}

pub(crate) fn empty_coinset_lists() -> (Vec<String>, Vec<String>, Vec<String>) {
    (Vec::new(), Vec::new(), Vec::new())
}

pub(crate) fn build_transition(args: TransitionBuildArgs<'_>) -> CycleOfferTransition {
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

pub(crate) fn build_transition_from_decision(
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
