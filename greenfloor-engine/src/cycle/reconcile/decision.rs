use super::cancel_submitted_policy::{resolve_cancel_submitted_transition, CancelSubmittedContext};
use chrono::{DateTime, Utc};

use super::builders::preserve_state;
use super::coinset_signals::CoinsetSignalSummary;
use super::dispatch::apply_watched_offer_dispatch;
use super::metadata::REASON_CANCEL_SUBMIT_CONTEXT_MISSING;
use super::state::ReconcileState;
use super::transition::ReconcileTransition;

pub(crate) fn resolve_watched_offer_decision(
    current_state: &ReconcileState,
    status: Option<i64>,
    summary: CoinsetSignalSummary,
    chain_confirmed_tx_ids: &[String],
    cancel_submitted: Option<&CancelSubmittedContext>,
    now: DateTime<Utc>,
) -> ReconcileTransition {
    if current_state.is_cancel_submitted() {
        let Some(ctx) = cancel_submitted else {
            return preserve_state(
                &ReconcileState::CancelSubmitted,
                REASON_CANCEL_SUBMIT_CONTEXT_MISSING,
            );
        };
        return resolve_cancel_submitted_transition(
            status,
            summary,
            chain_confirmed_tx_ids,
            ctx,
            now,
        );
    }
    apply_watched_offer_dispatch(summary, status, current_state)
}
