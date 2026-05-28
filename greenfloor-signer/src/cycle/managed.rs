use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::dispatch::{single_input_preferred_skip_reason, SpendableAssetProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedActionStatus {
    Executed,
    Skipped,
    PendingVisibility,
}

impl ManagedActionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Executed => "executed",
            Self::Skipped => "skipped",
            Self::PendingVisibility => "pending_visibility",
        }
    }

    pub fn is_pending_visibility(self) -> bool {
        matches!(self, Self::PendingVisibility)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedActionOutcome {
    pub status: ManagedActionStatus,
    pub reason: String,
    pub offer_id: Option<String>,
    pub transient_upstream: bool,
}

impl ManagedActionOutcome {
    pub fn is_pending_visibility(&self) -> bool {
        self.status.is_pending_visibility()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ParallelSubmissionDecision {
    Proceed {
        available_amounts: BTreeMap<String, i64>,
    },
    Skip {
        reason: String,
    },
}

pub fn is_transient_managed_upstream_error_text(error_text: &str) -> bool {
    let normalized = error_text.trim().to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "timed out",
        "timeout",
        "temporary unavailable",
        "temporarily unavailable",
        "bad gateway",
        "gateway timeout",
        "service unavailable",
        "connection reset",
        "connection refused",
        "managed_offer_http_error:502",
        "managed_offer_http_error:503",
        "managed_offer_http_error:504",
        "managed_offer_network_error",
        "signer_http_error:502",
        "signer_http_error:503",
        "signer_http_error:504",
    ];
    MARKERS.iter().any(|marker| normalized.contains(marker))
}

pub fn classify_managed_transient_error(
    exception_class: &str,
    _error_text: &str,
) -> Option<String> {
    match exception_class.trim() {
        "ReservationStorageError" => None,
        "ReservationContentionError" => Some("reservation_contention".to_string()),
        "ManagedUpstreamTransientError" => Some("upstream".to_string()),
        "TimeoutError" => Some("upstream".to_string()),
        _ => None,
    }
}

pub fn is_managed_upstream_transient_error(exception_class: &str, _error_text: &str) -> bool {
    exception_class.trim() == "ManagedUpstreamTransientError"
}

pub fn is_managed_worker_transient_error(exception_class: &str, error_text: &str) -> bool {
    classify_managed_transient_error(exception_class, error_text).as_deref() == Some("upstream")
}

pub fn is_parallel_dispatch_transient_error(exception_class: &str, error_text: &str) -> bool {
    matches!(
        classify_managed_transient_error(exception_class, error_text).as_deref(),
        Some("upstream") | Some("reservation_contention")
    )
}

pub fn is_transient_dexie_visibility_404_error(error: &str) -> bool {
    let normalized = error.trim().to_ascii_lowercase();
    (normalized.contains("dexie_get_offer_error") && normalized.contains("404"))
        || normalized.contains("dexie_http_error:404")
}

pub fn can_parallelize_managed_offers(
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> bool {
    signer_path_configured && parallelism_enabled && !runtime_dry_run && has_coordinator
}

pub fn parallel_max_workers(submission_count: usize, configured_max: usize) -> usize {
    submission_count.min(configured_max.max(1))
}

pub fn reservation_release_status(is_executed: bool) -> &'static str {
    if is_executed {
        "released_success"
    } else {
        "released_failed"
    }
}

pub fn should_apply_parallel_transient_cooldown(
    transient_failures: usize,
    total_parallel: usize,
    cooldown_seconds: u64,
) -> bool {
    if cooldown_seconds == 0 || total_parallel == 0 {
        return false;
    }
    let threshold = (total_parallel + 1) / 2;
    transient_failures >= threshold.max(2)
}

pub fn managed_retry_sleep_ms(attempt_index: u32, backoff_ms: u64) -> u64 {
    if backoff_ms == 0 {
        return 0;
    }
    backoff_ms.saturating_mul(1u64 << attempt_index.min(31))
}

pub fn should_retry_managed_post(
    attempt_index: u32,
    attempts_max: u32,
    is_upstream_transient: bool,
) -> bool {
    if !is_upstream_transient {
        return false;
    }
    let max_attempts = attempts_max.max(1);
    attempt_index + 1 < max_attempts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedRetryDecisionKind {
    Stop,
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedRetryDecision {
    pub decision: ManagedRetryDecisionKind,
    pub sleep_ms: u64,
}

pub fn managed_retry_decision(
    attempt_index: u32,
    attempts_max: u32,
    backoff_ms: u64,
    is_upstream_transient: bool,
) -> ManagedRetryDecision {
    if !should_retry_managed_post(attempt_index, attempts_max, is_upstream_transient) {
        return ManagedRetryDecision {
            decision: ManagedRetryDecisionKind::Stop,
            sleep_ms: 0,
        };
    }
    ManagedRetryDecision {
        decision: ManagedRetryDecisionKind::Retry,
        sleep_ms: managed_retry_sleep_ms(attempt_index, backoff_ms),
    }
}

pub fn prepare_parallel_managed_submission_decision(
    requested_amounts: &BTreeMap<String, i64>,
    spendable_profiles: &BTreeMap<String, SpendableAssetProfile>,
) -> ParallelSubmissionDecision {
    if requested_amounts.is_empty() {
        return ParallelSubmissionDecision::Skip {
            reason: "reservation_invalid_request".to_string(),
        };
    }
    if let Some(reason) =
        single_input_preferred_skip_reason(requested_amounts, spendable_profiles)
    {
        return ParallelSubmissionDecision::Skip { reason };
    }
    let available_amounts = requested_amounts
        .keys()
        .map(|asset_id| {
            let total = spendable_profiles
                .get(asset_id)
                .map(|profile| profile.total)
                .unwrap_or(0);
            (asset_id.clone(), total)
        })
        .collect();
    ParallelSubmissionDecision::Proceed { available_amounts }
}

pub fn classify_managed_post_result(
    success: bool,
    error_text: &str,
    offer_id: &str,
    publish_venue: &str,
) -> ManagedActionOutcome {
    if success {
        let clean_offer_id = offer_id.trim();
        if publish_venue.trim().eq_ignore_ascii_case("dexie") && !clean_offer_id.is_empty() {
            return ManagedActionOutcome {
                status: ManagedActionStatus::PendingVisibility,
                reason: "managed_offer_post_success".to_string(),
                offer_id: Some(clean_offer_id.to_string()),
                transient_upstream: false,
            };
        }
        return ManagedActionOutcome {
            status: ManagedActionStatus::Executed,
            reason: "managed_offer_post_success".to_string(),
            offer_id: if clean_offer_id.is_empty() {
                None
            } else {
                Some(clean_offer_id.to_string())
            },
            transient_upstream: false,
        };
    }
    ManagedActionOutcome {
        status: ManagedActionStatus::Skipped,
        reason: format!(
            "managed_offer_post_failed:{}",
            error_text.trim()
        ),
        offer_id: None,
        transient_upstream: false,
    }
}

pub fn classify_dexie_visibility_outcome(
    visible: bool,
    visibility_error: &str,
) -> ManagedActionOutcome {
    if visible {
        return ManagedActionOutcome {
            status: ManagedActionStatus::Executed,
            reason: "managed_offer_post_success".to_string(),
            offer_id: None,
            transient_upstream: false,
        };
    }
    if is_transient_dexie_visibility_404_error(visibility_error) {
        return ManagedActionOutcome {
            status: ManagedActionStatus::PendingVisibility,
            reason: "managed_offer_post_success".to_string(),
            offer_id: None,
            transient_upstream: false,
        };
    }
    ManagedActionOutcome {
        status: ManagedActionStatus::Skipped,
        reason: format!("managed_offer_post_not_visible_on_dexie:{visibility_error}"),
        offer_id: None,
        transient_upstream: false,
    }
}

pub fn count_parallel_transient_failures(items: &[(ManagedActionStatus, bool)]) -> usize {
    items
        .iter()
        .filter(|(status, transient_upstream)| {
            *status == ManagedActionStatus::Skipped && *transient_upstream
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_error_text_detects_timeout_markers() {
        assert!(is_transient_managed_upstream_error_text(
            "managed_offer_network_error: connection reset"
        ));
        assert!(!is_transient_managed_upstream_error_text("invalid offer"));
    }

    #[test]
    fn classify_timeout_as_upstream() {
        assert_eq!(
            classify_managed_transient_error("TimeoutError", "read timed out"),
            Some("upstream".to_string())
        );
    }

    #[test]
    fn classify_reservation_contention() {
        assert_eq!(
            classify_managed_transient_error("ReservationContentionError", "busy"),
            Some("reservation_contention".to_string())
        );
    }

    #[test]
    fn managed_upstream_retry_only_for_managed_exception_class() {
        assert!(is_managed_upstream_transient_error(
            "ManagedUpstreamTransientError",
            "timeout"
        ));
        assert!(!is_managed_upstream_transient_error("TimeoutError", "timeout"));
    }

    #[test]
    fn worker_transient_includes_timeout() {
        assert!(is_managed_worker_transient_error("TimeoutError", "timed out"));
    }

    #[test]
    fn parallel_cooldown_threshold() {
        assert!(should_apply_parallel_transient_cooldown(2, 2, 30));
        assert!(!should_apply_parallel_transient_cooldown(1, 2, 30));
        assert!(!should_apply_parallel_transient_cooldown(2, 2, 0));
    }

    #[test]
    fn managed_post_dexie_success_needs_visibility() {
        let outcome = classify_managed_post_result(true, "", "offer-1", "dexie");
        assert_eq!(outcome.status, ManagedActionStatus::PendingVisibility);
    }

    #[test]
    fn dexie_visibility_404_is_pending() {
        let outcome =
            classify_dexie_visibility_outcome(false, "dexie_http_error:404 not found");
        assert_eq!(outcome.status, ManagedActionStatus::PendingVisibility);
        assert_eq!(outcome.reason, "managed_offer_post_success");
    }

    #[test]
    fn managed_retry_decision_stops_when_not_transient() {
        let decision = managed_retry_decision(0, 3, 250, false);
        assert_eq!(decision.decision, ManagedRetryDecisionKind::Stop);
        assert_eq!(decision.sleep_ms, 0);
    }

    #[test]
    fn managed_retry_decision_retries_with_backoff() {
        let decision = managed_retry_decision(1, 3, 250, true);
        assert_eq!(decision.decision, ManagedRetryDecisionKind::Retry);
        assert_eq!(decision.sleep_ms, 500);
    }

    #[test]
    fn prepare_parallel_submission_skips_invalid_request() {
        let decision = prepare_parallel_managed_submission_decision(
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(
            decision,
            ParallelSubmissionDecision::Skip {
                reason: "reservation_invalid_request".to_string()
            }
        );
    }
}
