use crate::cycle::lifecycle::OfferSignal;
use crate::offer::dexie_payload::DEXIE_STATUS_CANCELLED;

use super::builders::{
    open_signal_transition, preserve_mempool_observation, preserve_state,
    transition_from_dexie_status,
};
use super::coinset_signals::CoinsetSignalSummary;
use super::metadata::{
    REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_COINSET_UNAVAILABLE,
    REASON_MISSING_STATUS, SIGNAL_SOURCE_COINSET_MEMPOOL, SIGNAL_SOURCE_COINSET_WEBSOCKET,
    TAKER_COINSET_TX_BLOCK_WEBSOCKET, TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_NONE,
};
use super::state::ReconcileState;
use super::transition::ReconcileTransition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusClass {
    Missing,
    Unavailable,
    Known(i64),
}

impl StatusClass {
    fn from_option(status: Option<i64>, has_coinset_tx_ids: bool) -> Self {
        match status {
            None if has_coinset_tx_ids => Self::Unavailable,
            None => Self::Missing,
            Some(code) => Self::Known(code),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileDispatch {
    CoinsetConfirmed,
    CoinsetMempool,
    MissingStatus,
    CoinsetUnavailable,
    DexieFallback(i64),
}

fn dispatch(
    coinset: CoinsetSignalSummary,
    status: StatusClass,
    current: &ReconcileState,
) -> ReconcileDispatch {
    let confirmed_eligible = coinset.has_confirmed
        && !matches!(status, StatusClass::Known(DEXIE_STATUS_CANCELLED))
        && !current.is_cancelled();
    if confirmed_eligible {
        return ReconcileDispatch::CoinsetConfirmed;
    }
    if coinset.has_mempool {
        return ReconcileDispatch::CoinsetMempool;
    }
    match status {
        StatusClass::Missing => ReconcileDispatch::MissingStatus,
        StatusClass::Unavailable => ReconcileDispatch::CoinsetUnavailable,
        StatusClass::Known(code) => ReconcileDispatch::DexieFallback(code),
    }
}

impl ReconcileDispatch {
    fn apply(self, current_state: &ReconcileState) -> ReconcileTransition {
        match self {
            Self::CoinsetConfirmed => open_signal_transition(
                OfferSignal::TxConfirmed,
                REASON_COINSET_CONFIRMED,
                SIGNAL_SOURCE_COINSET_WEBSOCKET,
                TAKER_COINSET_TX_BLOCK_WEBSOCKET,
                TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
            ),
            Self::CoinsetMempool => {
                if current_state.is_terminal() {
                    preserve_mempool_observation(current_state)
                } else {
                    open_signal_transition(
                        OfferSignal::MempoolSeen,
                        REASON_COINSET_MEMPOOL,
                        SIGNAL_SOURCE_COINSET_MEMPOOL,
                        TAKER_NONE,
                        TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
                    )
                }
            }
            Self::MissingStatus => preserve_state(current_state, REASON_MISSING_STATUS),
            Self::CoinsetUnavailable => preserve_state(current_state, REASON_COINSET_UNAVAILABLE),
            Self::DexieFallback(status) => {
                transition_from_dexie_status(status, current_state.clone())
            }
        }
    }
}

pub(crate) fn apply_watched_offer_dispatch(
    coinset: CoinsetSignalSummary,
    status: Option<i64>,
    current_state: &ReconcileState,
) -> ReconcileTransition {
    let status = StatusClass::from_option(status, coinset.has_coinset_activity());
    dispatch(coinset, status, current_state).apply(current_state)
}

pub(crate) fn apply_coinset_taker_dispatch_if_present(
    coinset: CoinsetSignalSummary,
    dexie_status: Option<i64>,
    current_state: &ReconcileState,
) -> Option<ReconcileTransition> {
    let status = StatusClass::from_option(dexie_status, coinset.has_coinset_activity());
    let selected = dispatch(coinset, status, current_state);
    match selected {
        ReconcileDispatch::CoinsetConfirmed | ReconcileDispatch::CoinsetMempool => {
            Some(selected.apply(current_state))
        }
        _ => None,
    }
}
