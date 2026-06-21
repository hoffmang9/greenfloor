use crate::cycle::lifecycle::OfferSignal;

use super::metadata::{
    REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_COINSET_UNAVAILABLE,
    REASON_DEXIE_OFFER_NOT_FOUND, REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL,
    REASON_MISSING_STATUS, REASON_OK, SIGNAL_SOURCE_COINSET_MEMPOOL, SIGNAL_SOURCE_COINSET_WEBHOOK,
    SIGNAL_SOURCE_DEXIE_GET_OFFER_404, SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK, SIGNAL_SOURCE_NONE,
    TAKER_COINSET_TX_BLOCK_WEBHOOK, TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK, TAKER_NONE,
};
use super::{
    decision::resolve_watched_offer_decision, resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals, unchanged_offer_transition,
    unsupported_venue_offer_transition, CycleOfferTransition, ReconcileState,
};

fn state(raw: &str) -> ReconcileState {
    ReconcileState::parse(raw).expect("valid reconcile state")
}

fn coinset_id(label: char) -> String {
    label.to_string().repeat(64)
}

#[derive(Clone, Copy)]
enum CoinsetFixture {
    Absent,
    TxOnly,
    Confirmed,
    Mempool,
    ConfirmedAndMempool,
}

impl CoinsetFixture {
    fn vecs(self) -> (Vec<String>, Vec<String>, Vec<String>) {
        match self {
            Self::Absent => (vec![], vec![], vec![]),
            Self::TxOnly => (vec![coinset_id('x')], vec![], vec![]),
            Self::Confirmed => (vec![coinset_id('x')], vec![coinset_id('c')], vec![]),
            Self::Mempool => (vec![coinset_id('x')], vec![], vec![coinset_id('m')]),
            Self::ConfirmedAndMempool => (
                vec![coinset_id('x')],
                vec![coinset_id('c')],
                vec![coinset_id('m')],
            ),
        }
    }
}

struct DispatchCase {
    label: &'static str,
    current_state: &'static str,
    status: Option<i64>,
    coinset: CoinsetFixture,
    expected_new_state: &'static str,
    expected_reason: &'static str,
    expected_signal_source: &'static str,
    expected_signal: Option<OfferSignal>,
    expected_changed: bool,
    expected_taker_signal: &'static str,
    expected_taker_diagnostic: &'static str,
}

const DISPATCH_CASES: &[DispatchCase] = &[
    DispatchCase {
        label: "coinset_confirmed_moves_open_offer_to_tx_block_confirmed",
        current_state: "open",
        status: Some(0),
        coinset: CoinsetFixture::Confirmed,
        expected_new_state: "tx_block_confirmed",
        expected_reason: REASON_COINSET_CONFIRMED,
        expected_signal_source: SIGNAL_SOURCE_COINSET_WEBHOOK,
        expected_signal: Some(OfferSignal::TxConfirmed),
        expected_changed: true,
        expected_taker_signal: TAKER_COINSET_TX_BLOCK_WEBHOOK,
        expected_taker_diagnostic: TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    },
    DispatchCase {
        label: "coinset_mempool_moves_open_offer_to_mempool_observed",
        current_state: "open",
        status: Some(0),
        coinset: CoinsetFixture::Mempool,
        expected_new_state: "mempool_observed",
        expected_reason: REASON_COINSET_MEMPOOL,
        expected_signal_source: SIGNAL_SOURCE_COINSET_MEMPOOL,
        expected_signal: Some(OfferSignal::MempoolSeen),
        expected_changed: true,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
    },
    DispatchCase {
        label: "dexie_fallback_preserves_open_when_no_coinset_signal",
        current_state: "open",
        status: Some(0),
        coinset: CoinsetFixture::TxOnly,
        expected_new_state: "open",
        expected_reason: REASON_OK,
        expected_signal_source: SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
        expected_signal: None,
        expected_changed: false,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_NONE,
    },
    DispatchCase {
        label: "missing_status_without_tx_ids",
        current_state: "open",
        status: None,
        coinset: CoinsetFixture::Absent,
        expected_new_state: "open",
        expected_reason: REASON_MISSING_STATUS,
        expected_signal_source: SIGNAL_SOURCE_NONE,
        expected_signal: None,
        expected_changed: false,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_NONE,
    },
    DispatchCase {
        label: "coinset_signal_unavailable_for_offer",
        current_state: "open",
        status: None,
        coinset: CoinsetFixture::TxOnly,
        expected_new_state: "open",
        expected_reason: REASON_COINSET_UNAVAILABLE,
        expected_signal_source: SIGNAL_SOURCE_NONE,
        expected_signal: None,
        expected_changed: false,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_NONE,
    },
    DispatchCase {
        label: "dexie_status_fallback_when_no_coinset_tx_ids",
        current_state: "open",
        status: Some(4),
        coinset: CoinsetFixture::Absent,
        expected_new_state: "tx_block_confirmed",
        expected_reason: REASON_OK,
        expected_signal_source: SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
        expected_signal: Some(OfferSignal::TxConfirmed),
        expected_changed: true,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK,
    },
    DispatchCase {
        label: "dexie_cancelled_status_fallback",
        current_state: "open",
        status: Some(3),
        coinset: CoinsetFixture::Absent,
        expected_new_state: "cancelled",
        expected_reason: REASON_OK,
        expected_signal_source: SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
        expected_signal: None,
        expected_changed: true,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_NONE,
    },
    DispatchCase {
        label: "decision_preserves_terminal_state_on_mempool_signal",
        current_state: "tx_block_confirmed",
        status: Some(0),
        coinset: CoinsetFixture::Mempool,
        expected_new_state: "tx_block_confirmed",
        expected_reason: REASON_COINSET_MEMPOOL,
        expected_signal_source: SIGNAL_SOURCE_COINSET_MEMPOOL,
        expected_signal: None,
        expected_changed: false,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
    },
    DispatchCase {
        label: "decision_skips_coinset_confirmed_when_offer_is_cancelled",
        current_state: "cancelled",
        status: Some(0),
        coinset: CoinsetFixture::Confirmed,
        expected_new_state: "cancelled",
        expected_reason: REASON_OK,
        expected_signal_source: SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
        expected_signal: None,
        expected_changed: false,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_NONE,
    },
    DispatchCase {
        label: "decision_skips_coinset_confirmed_when_dexie_status_is_cancelled",
        current_state: "open",
        status: Some(3),
        coinset: CoinsetFixture::Confirmed,
        expected_new_state: "cancelled",
        expected_reason: REASON_OK,
        expected_signal_source: SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
        expected_signal: None,
        expected_changed: true,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_NONE,
    },
    DispatchCase {
        label: "decision_confirmed_blocked_by_dexie_cancelled_falls_through_to_mempool",
        current_state: "open",
        status: Some(3),
        coinset: CoinsetFixture::ConfirmedAndMempool,
        expected_new_state: "mempool_observed",
        expected_reason: REASON_COINSET_MEMPOOL,
        expected_signal_source: SIGNAL_SOURCE_COINSET_MEMPOOL,
        expected_signal: Some(OfferSignal::MempoolSeen),
        expected_changed: true,
        expected_taker_signal: TAKER_NONE,
        expected_taker_diagnostic: TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
    },
];

fn run_dispatch_case(case: &DispatchCase) -> CycleOfferTransition {
    let current = state(case.current_state);
    let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) = case.coinset.vecs();
    resolve_watched_offer_decision(
        &current,
        case.status,
        &coinset_tx_ids,
        &coinset_confirmed_tx_ids,
        &coinset_mempool_tx_ids,
    )
    .into_cycle_transition_no_coinset(current)
}

fn assert_dispatch_case(case: &DispatchCase) {
    let transition = run_dispatch_case(case);
    assert_eq!(
        transition.new_state,
        state(case.expected_new_state),
        "{}: new_state",
        case.label
    );
    assert_eq!(
        transition.reason, case.expected_reason,
        "{}: reason",
        case.label
    );
    assert_eq!(
        transition.signal_source, case.expected_signal_source,
        "{}: signal_source",
        case.label
    );
    assert_eq!(
        transition.signal, case.expected_signal,
        "{}: signal",
        case.label
    );
    assert_eq!(
        transition.changed, case.expected_changed,
        "{}: changed",
        case.label
    );
    assert_eq!(
        transition.taker_signal, case.expected_taker_signal,
        "{}: taker_signal",
        case.label
    );
    assert_eq!(
        transition.taker_diagnostic, case.expected_taker_diagnostic,
        "{}: taker_diagnostic",
        case.label
    );
}

#[test]
fn watched_offer_dispatch_matrix() {
    for case in DISPATCH_CASES {
        assert_dispatch_case(case);
    }
}

#[test]
fn resolve_watched_offer_transition_from_signals_matches_dispatch_matrix() {
    for case in DISPATCH_CASES {
        let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) =
            case.coinset.vecs();
        let transition = resolve_watched_offer_transition_from_signals(
            case.current_state,
            case.status,
            coinset_tx_ids,
            coinset_confirmed_tx_ids,
            coinset_mempool_tx_ids,
        )
        .unwrap_or_else(|err| panic!("{}: valid reconcile state: {err}", case.label));
        assert_eq!(
            transition.old_state,
            state(case.current_state),
            "{}: old_state",
            case.label
        );
        assert_eq!(
            transition.new_state,
            state(case.expected_new_state),
            "{}: new_state",
            case.label
        );
        assert_eq!(
            transition.reason, case.expected_reason,
            "{}: reason",
            case.label
        );
        assert_eq!(
            transition.signal_source, case.expected_signal_source,
            "{}: signal_source",
            case.label
        );
        assert_eq!(
            transition.signal, case.expected_signal,
            "{}: signal",
            case.label
        );
        assert_eq!(
            transition.changed, case.expected_changed,
            "{}: changed",
            case.label
        );
        assert_eq!(
            transition.taker_signal, case.expected_taker_signal,
            "{}: taker_signal",
            case.label
        );
        assert_eq!(
            transition.taker_diagnostic, case.expected_taker_diagnostic,
            "{}: taker_diagnostic",
            case.label
        );
    }
}

struct MissingWatchedCase {
    label: &'static str,
    current_state: &'static str,
    expected_new_state: &'static str,
    expected_changed: bool,
    expected_immediate_requeue: bool,
    expected_signal: Option<OfferSignal>,
    expected_reason: &'static str,
    expected_signal_source: &'static str,
}

const MISSING_WATCHED_CASES: &[MissingWatchedCase] = &[
    MissingWatchedCase {
        label: "missing_watched_offer_expires_open_offer",
        current_state: "open",
        expected_new_state: "expired",
        expected_changed: true,
        expected_immediate_requeue: true,
        expected_signal: Some(OfferSignal::Expired),
        expected_reason: REASON_DEXIE_OFFER_NOT_FOUND,
        expected_signal_source: SIGNAL_SOURCE_DEXIE_GET_OFFER_404,
    },
    MissingWatchedCase {
        label: "missing_watched_offer_preserves_terminal_state",
        current_state: "tx_block_confirmed",
        expected_new_state: "tx_block_confirmed",
        expected_changed: false,
        expected_immediate_requeue: false,
        expected_signal: None,
        expected_reason: REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL,
        expected_signal_source: SIGNAL_SOURCE_DEXIE_GET_OFFER_404,
    },
];

#[test]
fn missing_watched_offer_matrix() {
    for case in MISSING_WATCHED_CASES {
        let transition = resolve_missing_watched_offer_transition(case.current_state)
            .unwrap_or_else(|err| panic!("{}: valid reconcile state: {err}", case.label));
        assert_eq!(
            transition.new_state,
            state(case.expected_new_state),
            "{}: new_state",
            case.label
        );
        assert_eq!(
            transition.changed, case.expected_changed,
            "{}: changed",
            case.label
        );
        assert_eq!(
            transition.immediate_requeue, case.expected_immediate_requeue,
            "{}: immediate_requeue",
            case.label
        );
        assert_eq!(
            transition.signal, case.expected_signal,
            "{}: signal",
            case.label
        );
        assert_eq!(
            transition.reason, case.expected_reason,
            "{}: reason",
            case.label
        );
        assert_eq!(
            transition.signal_source, case.expected_signal_source,
            "{}: signal_source",
            case.label
        );
    }
}

struct FactoryCase {
    label: &'static str,
    current_state: &'static str,
    expected_new_state: &'static str,
    expected_changed: bool,
}

const FACTORY_CASES: &[FactoryCase] = &[FactoryCase {
    label: "unchanged_offer_transition_factory",
    current_state: "open",
    expected_new_state: "open",
    expected_changed: false,
}];

#[test]
fn entry_point_factory_matrix() {
    for case in FACTORY_CASES {
        let transition = unchanged_offer_transition(case.current_state, "dexie_lookup_error:boom")
            .unwrap_or_else(|err| panic!("{}: valid reconcile state: {err}", case.label));
        assert_eq!(
            transition.old_state,
            state(case.current_state),
            "{}: old_state",
            case.label
        );
        assert_eq!(
            transition.new_state,
            state(case.expected_new_state),
            "{}: new_state",
            case.label
        );
        assert_eq!(
            transition.changed, case.expected_changed,
            "{}: changed",
            case.label
        );
        assert_eq!(
            transition.taker_signal, TAKER_NONE,
            "{}: taker_signal",
            case.label
        );
    }

    let transition =
        unsupported_venue_offer_transition("open", "splash").expect("valid reconcile state");
    assert_eq!(transition.new_state, state("reconcile_unsupported_venue"));
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
