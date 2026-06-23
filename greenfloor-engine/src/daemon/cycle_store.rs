//! Shared cycle sqlite access patterns for the daemon.
//!
//! One [`CycleWriteStore`] is opened per daemon cycle and threaded through market dispatch.
//! Choose the access mode by how long the caller holds the store:
//!
//! - [`CycleWriteStore::sync`] — short synchronous reads/writes (strategy planning, fallback
//!   logging, reservation audits). Release before parallel offer-dispatch work.
//! - [`CycleWriteStore::lock`] — brief parallel writes (managed-offer persist flush, coordinator
//!   reservation updates). Do not hold across network or build-and-post work.
//! - [`cycle_locked!`] / [`locked_logged_phase!`] — hold the lock across one async sequential
//!   market phase (inventory, cancel, `coin_ops`, reconcile, preamble, plan). Use only on the
//!   per-market sequential path; parallel workers must not take this lock.
//!
//! Do not hold a cycle lock while parallel offer-dispatch workers run build-and-post; strategy
//! releases the lock before dispatch.

use std::future::Future;

use crate::error::SignerResult;
use crate::operator_log::{LogContext, MARKET_PHASE};
use tracing::Level;

#[derive(Clone, Copy)]
enum MarketPhaseTraceOutcome {
    Started,
    Failed,
    Completed,
}

impl MarketPhaseTraceOutcome {
    const fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }
}

fn trace_market_phase(
    market_id: &str,
    market_phase: &str,
    outcome: MarketPhaseTraceOutcome,
    level: Level,
) {
    macro_rules! emit {
        ($msg:literal) => {
            crate::trace_event!(
                level = level,
                LogContext::MARKET_CYCLE,
                MARKET_PHASE,
                {
                    market_id = market_id,
                    market_phase = market_phase,
                    outcome = outcome.label(),
                };
                $msg
            );
        };
    }
    match outcome {
        MarketPhaseTraceOutcome::Started => {
            emit!("market phase started");
        }
        MarketPhaseTraceOutcome::Failed => {
            emit!("market phase failed");
        }
        MarketPhaseTraceOutcome::Completed => {
            emit!("market phase completed");
        }
    }
}

pub(crate) async fn run_logged_market_phase<F, T>(
    market_id: &str,
    market_phase: &str,
    body: F,
) -> SignerResult<T>
where
    F: Future<Output = SignerResult<T>>,
{
    trace_market_phase(
        market_id,
        market_phase,
        MarketPhaseTraceOutcome::Started,
        Level::DEBUG,
    );
    let result = body.await;
    if result.is_err() {
        trace_market_phase(
            market_id,
            market_phase,
            MarketPhaseTraceOutcome::Failed,
            Level::WARN,
        );
    } else {
        trace_market_phase(
            market_id,
            market_phase,
            MarketPhaseTraceOutcome::Completed,
            Level::DEBUG,
        );
    }
    result
}

/// Run one async sequential market phase under the cycle-store lock with phase tracing.
#[macro_export]
macro_rules! locked_logged_phase {
    ($market_id:expr, $phase:expr, $write_store:expr, |$guard:ident| $body:expr) => {{
        $crate::daemon::cycle_store::run_logged_market_phase($market_id, $phase, async {
            $crate::cycle_locked!($write_store, |$guard| $body)
        })
    }};
}
