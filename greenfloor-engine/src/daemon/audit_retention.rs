use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use chrono::{Duration, Utc};

use crate::error::SignerResult;
use crate::storage::SqliteStore;
use crate::storage::DEFAULT_AUDIT_PRUNE_INTERVAL_SECONDS;

static NEXT_PRUNE_DEADLINE: OnceLock<Mutex<Option<f64>>> = OnceLock::new();

fn monotonic_seconds() -> f64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn next_prune_deadline(now: f64, interval_seconds: u64) -> f64 {
    now + crate::offer::pricing::u64_to_f64(interval_seconds.max(1))
}

pub fn audit_prune_interval_seconds() -> u64 {
    std::env::var("GREENFLOOR_AUDIT_PRUNE_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_AUDIT_PRUNE_INTERVAL_SECONDS)
        .max(3_600)
}

pub fn maybe_prune_audit_events(
    store: &SqliteStore,
    retention_days: u64,
) -> SignerResult<Option<u64>> {
    let interval_seconds = audit_prune_interval_seconds();
    let now = monotonic_seconds();
    let mutex = NEXT_PRUNE_DEADLINE.get_or_init(|| Mutex::new(None));
    let Ok(mut next_deadline) = mutex.lock() else {
        return Ok(None);
    };
    let deadline = next_deadline.unwrap_or(0.0);
    if now < deadline {
        return Ok(None);
    }
    *next_deadline = Some(next_prune_deadline(now, interval_seconds));

    let cutoff = Utc::now() - Duration::days(i64::try_from(retention_days).unwrap_or(i64::MAX));
    let deleted = store.prune_audit_events_older_than(cutoff)?;
    if deleted > 0 {
        tracing::info!(
            deleted,
            retention_days,
            cutoff = %cutoff.to_rfc3339(),
            interval_seconds,
            event = "audit_event_pruned",
            "pruned non-financial audit events"
        );
    }
    Ok(Some(deleted))
}
