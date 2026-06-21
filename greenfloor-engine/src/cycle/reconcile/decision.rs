use crate::cycle::lifecycle::OfferSignal;
use crate::offer::dexie_payload::{
    is_dexie_pattern_fallback_status, reconcile_from_dexie_status, DexieStatusReconcile,
    DEXIE_STATUS_CANCELLED,
};

use super::state::{ReconcileState, TAKER_NONE};
use super::transition::ReconcileTransition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoinsetSignals {
    Both,
    Confirmed,
    Mempool,
    None,
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

impl CoinsetSignals {
    fn from_presence(has_confirmed: bool, has_mempool: bool) -> Self {
        match (has_confirmed, has_mempool) {
            (true, true) => Self::Both,
            (true, false) => Self::Confirmed,
            (false, true) => Self::Mempool,
            (false, false) => Self::None,
        }
    }

    fn has_confirmed(self) -> bool {
        matches!(self, Self::Confirmed | Self::Both)
    }

    fn has_mempool(self) -> bool {
        matches!(self, Self::Mempool | Self::Both)
    }
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
    coinset: CoinsetSignals,
    status: StatusClass,
    current_is_cancelled: bool,
) -> ReconcileDispatch {
    let confirmed_eligible = coinset.has_confirmed()
        && !matches!(status, StatusClass::Known(DEXIE_STATUS_CANCELLED))
        && !current_is_cancelled;
    if confirmed_eligible {
        return ReconcileDispatch::CoinsetConfirmed;
    }
    if coinset.has_mempool() {
        return ReconcileDispatch::CoinsetMempool;
    }
    match status {
        StatusClass::Missing => ReconcileDispatch::MissingStatus,
        StatusClass::Unavailable => ReconcileDispatch::CoinsetUnavailable,
        StatusClass::Known(code) => ReconcileDispatch::DexieFallback(code),
    }
}

fn coinset_confirmed() -> ReconcileTransition {
    let (new_state, signal) = ReconcileState::from_open_signal(OfferSignal::TxConfirmed);
    ReconcileTransition::new(
        new_state,
        "coinset_tx_block_webhook_confirmed",
        "coinset_webhook",
        Some(signal),
        "coinset_tx_block_webhook",
        "coinset_tx_block_confirmed",
    )
}

fn coinset_mempool(current_state: &ReconcileState) -> ReconcileTransition {
    if current_state.is_terminal() {
        return ReconcileTransition::new(
            current_state.clone(),
            "coinset_mempool_observed",
            "coinset_mempool",
            None,
            TAKER_NONE,
            "coinset_mempool_observed",
        );
    }
    let (new_state, signal) = ReconcileState::from_open_signal(OfferSignal::MempoolSeen);
    ReconcileTransition::new(
        new_state,
        "coinset_mempool_observed",
        "coinset_mempool",
        Some(signal),
        TAKER_NONE,
        "coinset_mempool_observed",
    )
}

fn preserve_current_state(
    current_state: &ReconcileState,
    reason: &'static str,
) -> ReconcileTransition {
    ReconcileTransition::new(
        current_state.clone(),
        reason,
        "none",
        None,
        TAKER_NONE,
        TAKER_NONE,
    )
}

fn dexie_fallback(current_state: &ReconcileState, status: i64) -> ReconcileTransition {
    let taker_diagnostic = if is_dexie_pattern_fallback_status(status) {
        "dexie_status_pattern_fallback"
    } else {
        TAKER_NONE
    };
    match reconcile_from_dexie_status(status) {
        DexieStatusReconcile::Cancelled => ReconcileTransition::new(
            ReconcileState::Cancelled,
            "ok",
            "dexie_status_fallback",
            None,
            TAKER_NONE,
            taker_diagnostic,
        ),
        DexieStatusReconcile::Lifecycle { signal, new_state } => {
            let new_state = ReconcileState::Lifecycle(new_state);
            let signal = if new_state == *current_state {
                None
            } else {
                Some(signal)
            };
            ReconcileTransition::new(
                new_state,
                "ok",
                "dexie_status_fallback",
                signal,
                TAKER_NONE,
                taker_diagnostic,
            )
        }
        DexieStatusReconcile::Unchanged => ReconcileTransition::new(
            current_state.clone(),
            "ok",
            "dexie_status_fallback",
            None,
            TAKER_NONE,
            taker_diagnostic,
        ),
    }
}

pub(crate) fn resolve_watched_offer_decision(
    current_state: &ReconcileState,
    status: Option<i64>,
    coinset_tx_ids: &[String],
    coinset_confirmed_tx_ids: &[String],
    coinset_mempool_tx_ids: &[String],
) -> ReconcileTransition {
    let coinset = CoinsetSignals::from_presence(
        !coinset_confirmed_tx_ids.is_empty(),
        !coinset_mempool_tx_ids.is_empty(),
    );
    let status = StatusClass::from_option(status, !coinset_tx_ids.is_empty());

    match dispatch(coinset, status, current_state.is_cancelled()) {
        ReconcileDispatch::CoinsetConfirmed => coinset_confirmed(),
        ReconcileDispatch::CoinsetMempool => coinset_mempool(current_state),
        ReconcileDispatch::MissingStatus => preserve_current_state(current_state, "missing_status"),
        ReconcileDispatch::CoinsetUnavailable => {
            preserve_current_state(current_state, "coinset_signal_unavailable_for_offer")
        }
        ReconcileDispatch::DexieFallback(status) => dexie_fallback(current_state, status),
    }
}
