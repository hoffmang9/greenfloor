use chrono::{DateTime, Utc};

pub(crate) const RESEED_MEMPOOL_MAX_AGE_SECONDS: i64 = 3 * 60;
pub(crate) const PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS: i64 = 2 * 60;

pub(crate) fn parse_event_created_at(value: &str) -> Option<DateTime<Utc>> {
    let raw = value.trim();
    if raw.is_empty() {
        return None;
    }
    let normalized = raw.replace('Z', "+00:00");
    DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|naive| naive.and_utc())
        })
}

pub(crate) fn is_recent_mempool_observed_offer_state(updated_at: &str, clock: DateTime<Utc>) -> bool {
    let Some(parsed) = parse_event_created_at(updated_at) else {
        return false;
    };
    let age_seconds = (clock - parsed).num_seconds();
    (0..=RESEED_MEMPOOL_MAX_AGE_SECONDS).contains(&age_seconds)
}
