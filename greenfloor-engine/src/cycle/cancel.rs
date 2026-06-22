const DEFAULT_CANCEL_MOVE_THRESHOLD_BPS: i64 = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct CancelPolicyDecision {
    pub eligible: bool,
    pub triggered: bool,
    pub reason: String,
    pub move_bps: Option<f64>,
    pub threshold_bps: i64,
}

#[must_use]
pub fn abs_move_bps(current: Option<f64>, previous: Option<f64>) -> Option<f64> {
    let current = current?;
    let previous = previous?;
    if current <= 0.0 || previous <= 0.0 {
        return None;
    }
    Some(((current - previous) / previous).abs() * 10_000.0)
}

#[must_use]
pub fn cancel_move_threshold_bps(market_threshold: Option<i64>, env_threshold: Option<i64>) -> i64 {
    if let Some(threshold) = market_threshold {
        if threshold > 0 {
            return threshold;
        }
    }
    if let Some(threshold) = env_threshold {
        return threshold.max(1);
    }
    DEFAULT_CANCEL_MOVE_THRESHOLD_BPS
}

#[must_use]
pub fn evaluate_cancel_policy_decision(
    quote_asset_type: &str,
    cancel_policy_stable_vs_unstable: bool,
    current_xch_price_usd: Option<f64>,
    previous_xch_price_usd: Option<f64>,
    market_threshold: Option<i64>,
    env_threshold: Option<i64>,
) -> CancelPolicyDecision {
    let move_bps = abs_move_bps(current_xch_price_usd, previous_xch_price_usd);
    let threshold_bps = cancel_move_threshold_bps(market_threshold, env_threshold);
    let quote_type = quote_asset_type.trim().to_ascii_lowercase();

    if quote_type != "unstable" {
        return CancelPolicyDecision {
            eligible: false,
            triggered: false,
            reason: "not_unstable_leg_market".to_string(),
            move_bps,
            threshold_bps,
        };
    }
    if !cancel_policy_stable_vs_unstable {
        return CancelPolicyDecision {
            eligible: false,
            triggered: false,
            reason: "not_stable_vs_unstable_market".to_string(),
            move_bps,
            threshold_bps,
        };
    }
    if move_bps.is_none() {
        return CancelPolicyDecision {
            eligible: true,
            triggered: false,
            reason: "missing_price_baseline".to_string(),
            move_bps: None,
            threshold_bps,
        };
    }
    let Some(move_bps) = move_bps else {
        unreachable!("move_bps checked above");
    };
    if move_bps < crate::offer::pricing::i64_to_f64(threshold_bps) {
        return CancelPolicyDecision {
            eligible: true,
            triggered: false,
            reason: "price_move_below_threshold".to_string(),
            move_bps: Some(move_bps),
            threshold_bps,
        };
    }

    CancelPolicyDecision {
        eligible: true,
        triggered: true,
        reason: "strong_unstable_price_move".to_string(),
        move_bps: Some(move_bps),
        threshold_bps,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        abs_move_bps, cancel_move_threshold_bps, evaluate_cancel_policy_decision,
    };

    #[test]
    fn abs_move_bps_positive_move() {
        let result = abs_move_bps(Some(110.0), Some(100.0)).expect("move");
        assert!((result - 1000.0).abs() < 0.01);
    }

    #[test]
    fn abs_move_bps_rejects_missing_or_non_positive() {
        assert!(abs_move_bps(None, Some(100.0)).is_none());
        assert!(abs_move_bps(Some(100.0), None).is_none());
        assert!(abs_move_bps(Some(0.0), Some(100.0)).is_none());
        assert!(abs_move_bps(Some(100.0), Some(0.0)).is_none());
    }

    #[test]
    fn threshold_prefers_market_override() {
        assert_eq!(cancel_move_threshold_bps(Some(100), Some(250)), 100);
    }

    #[test]
    fn threshold_uses_env_when_market_missing() {
        assert_eq!(cancel_move_threshold_bps(None, Some(250)), 250);
        assert_eq!(cancel_move_threshold_bps(None, None), 500);
        assert_eq!(cancel_move_threshold_bps(Some(0), None), 500);
    }

    #[test]
    fn skips_non_unstable_market() {
        let decision =
            evaluate_cancel_policy_decision("stable", true, Some(30.0), Some(25.0), None, None);
        assert!(!decision.eligible);
        assert_eq!(decision.reason, "not_unstable_leg_market");
    }

    #[test]
    fn requires_stable_vs_unstable_flag() {
        let decision =
            evaluate_cancel_policy_decision("unstable", false, Some(45.0), Some(30.0), None, None);
        assert!(!decision.eligible);
        assert_eq!(decision.reason, "not_stable_vs_unstable_market");
    }

    #[test]
    fn requires_strong_price_move() {
        let decision =
            evaluate_cancel_policy_decision("unstable", true, Some(30.2), Some(30.0), None, None);
        assert!(decision.eligible);
        assert!(!decision.triggered);
        assert_eq!(decision.reason, "price_move_below_threshold");
    }

    #[test]
    fn triggers_on_market_specific_threshold() {
        let decision = evaluate_cancel_policy_decision(
            "unstable",
            true,
            Some(30.6),
            Some(30.0),
            Some(100),
            None,
        );
        assert!(decision.triggered);
        assert_eq!(decision.threshold_bps, 100);
    }
}
