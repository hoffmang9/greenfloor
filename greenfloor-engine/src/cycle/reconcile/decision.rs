use crate::cycle::lifecycle::OfferSignal;
use crate::offer::dexie_payload::{
    is_dexie_pattern_fallback_status, reconcile_from_dexie_status, DexieStatusReconcile,
    DEXIE_STATUS_CANCELLED,
};

use super::builders::{
    dexie_fallback_transition, open_signal_transition, preserve_mempool_observation, preserve_state,
};
use super::metadata::{
    REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_COINSET_UNAVAILABLE,
    REASON_MISSING_STATUS, SIGNAL_SOURCE_COINSET_MEMPOOL, SIGNAL_SOURCE_COINSET_WEBHOOK,
    TAKER_COINSET_TX_BLOCK_WEBHOOK, TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK, TAKER_NONE,
};
use super::state::ReconcileState;
use super::transition::ReconcileTransition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CoinsetPresence {
    has_confirmed: bool,
    has_mempool: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusClass {
    Missing,
    Unavailable,
    Known(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconcileDispatch {
    CoinsetConfirmed,
    CoinsetMempool,
    MissingStatus,
    CoinsetUnavailable,
    DexieFallback(i64),
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

fn dispatch(
    coinset: CoinsetPresence,
    status: StatusClass,
    current_is_cancelled: bool,
) -> ReconcileDispatch {
    let confirmed_eligible = coinset.has_confirmed
        && !matches!(status, StatusClass::Known(DEXIE_STATUS_CANCELLED))
        && !current_is_cancelled;
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
                SIGNAL_SOURCE_COINSET_WEBHOOK,
                TAKER_COINSET_TX_BLOCK_WEBHOOK,
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
                let taker_diagnostic = if is_dexie_pattern_fallback_status(status) {
                    TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK
                } else {
                    TAKER_NONE
                };
                match reconcile_from_dexie_status(status) {
                    DexieStatusReconcile::Cancelled => {
                        dexie_fallback_transition(ReconcileState::Cancelled, None, taker_diagnostic)
                    }
                    DexieStatusReconcile::ApplySignal(signal) => dexie_fallback_transition(
                        ReconcileState::from_open_signal(signal),
                        Some(signal),
                        taker_diagnostic,
                    ),
                    DexieStatusReconcile::Unchanged => {
                        dexie_fallback_transition(current_state.clone(), None, taker_diagnostic)
                    }
                }
            }
        }
    }
}

pub(crate) fn resolve_watched_offer_decision(
    current_state: &ReconcileState,
    status: Option<i64>,
    coinset_tx_ids: &[String],
    coinset_confirmed_tx_ids: &[String],
    coinset_mempool_tx_ids: &[String],
) -> ReconcileTransition {
    let coinset = CoinsetPresence {
        has_confirmed: !coinset_confirmed_tx_ids.is_empty(),
        has_mempool: !coinset_mempool_tx_ids.is_empty(),
    };
    let status = StatusClass::from_option(status, !coinset_tx_ids.is_empty());
    dispatch(coinset, status, current_state.is_cancelled()).apply(current_state)
}
