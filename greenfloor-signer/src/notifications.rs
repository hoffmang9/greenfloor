#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertEvent {
    pub market_id: String,
    pub ticker: String,
    pub remaining_amount: i64,
    pub receive_address: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertState {
    pub is_low: bool,
    pub last_alert_at_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowInventoryEvaluation {
    pub state: AlertState,
    pub event: Option<AlertEvent>,
}

pub fn compute_low_inventory_threshold(
    market_threshold: Option<i64>,
    program_default_threshold: i64,
    low_watermark: i64,
) -> i64 {
    if let Some(threshold) = market_threshold {
        return threshold;
    }
    if program_default_threshold > 0 {
        return program_default_threshold;
    }
    low_watermark
}

pub fn evaluate_low_inventory_alert(
    now_unix: i64,
    low_inventory_enabled: bool,
    program_default_threshold: i64,
    clear_hysteresis_percent: f64,
    dedup_cooldown_seconds: i64,
    market_enabled: bool,
    market_id: &str,
    ticker: &str,
    receive_address: &str,
    market_threshold: Option<i64>,
    low_watermark: i64,
    remaining: i64,
    state_is_low: bool,
    state_last_alert_at_unix: Option<i64>,
) -> LowInventoryEvaluation {
    if !market_enabled || !low_inventory_enabled {
        return LowInventoryEvaluation {
            state: AlertState {
                is_low: state_is_low,
                last_alert_at_unix: state_last_alert_at_unix,
            },
            event: None,
        };
    }

    let threshold =
        compute_low_inventory_threshold(market_threshold, program_default_threshold, low_watermark);
    let hysteresis_target =
        ((threshold as f64) * (1.0 + clear_hysteresis_percent / 100.0)).floor() as i64;

    let mut next_state = AlertState {
        is_low: state_is_low,
        last_alert_at_unix: state_last_alert_at_unix,
    };

    if remaining >= hysteresis_target {
        next_state.is_low = false;
        return LowInventoryEvaluation {
            state: next_state,
            event: None,
        };
    }

    if remaining >= threshold {
        return LowInventoryEvaluation {
            state: next_state,
            event: None,
        };
    }

    let mut should_send = false;
    let mut reason = "low_triggered".to_string();
    if !state_is_low {
        should_send = true;
    } else if state_last_alert_at_unix.is_none() {
        should_send = true;
    } else if now_unix.saturating_sub(state_last_alert_at_unix.unwrap_or(now_unix))
        >= dedup_cooldown_seconds
    {
        should_send = true;
        reason = "reminder_sent".to_string();
    }

    next_state.is_low = true;
    if should_send {
        next_state.last_alert_at_unix = Some(now_unix);
        return LowInventoryEvaluation {
            state: next_state,
            event: Some(AlertEvent {
                market_id: market_id.to_string(),
                ticker: ticker.to_string(),
                remaining_amount: remaining,
                receive_address: receive_address.to_string(),
                reason,
            }),
        };
    }

    LowInventoryEvaluation {
        state: next_state,
        event: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_low_inventory_threshold, evaluate_low_inventory_alert, AlertState,
        LowInventoryEvaluation,
    };

    #[test]
    fn threshold_prefers_market_override() {
        assert_eq!(compute_low_inventory_threshold(Some(42), 10, 100), 42);
    }

    #[test]
    fn threshold_falls_back_to_program_default() {
        assert_eq!(compute_low_inventory_threshold(None, 75, 100), 75);
    }

    #[test]
    fn threshold_uses_low_watermark_when_no_defaults() {
        assert_eq!(compute_low_inventory_threshold(None, 0, 100), 100);
    }

    #[test]
    fn first_low_inventory_alert_fires() {
        let result = evaluate_low_inventory_alert(
            1_000, true, 0, 10.0, 3_600, true, "market-a", "BYC", "xch1addr", None, 100, 90, false,
            None,
        );
        assert_eq!(
            result,
            LowInventoryEvaluation {
                state: AlertState {
                    is_low: true,
                    last_alert_at_unix: Some(1_000),
                },
                event: Some(super::AlertEvent {
                    market_id: "market-a".to_string(),
                    ticker: "BYC".to_string(),
                    remaining_amount: 90,
                    receive_address: "xch1addr".to_string(),
                    reason: "low_triggered".to_string(),
                }),
            }
        );
    }

    #[test]
    fn dedup_respects_cooldown() {
        let result = evaluate_low_inventory_alert(
            2_000,
            true,
            0,
            10.0,
            3_600,
            true,
            "market-a",
            "BYC",
            "xch1addr",
            None,
            100,
            80,
            true,
            Some(1_000),
        );
        assert!(result.state.is_low);
        assert!(result.event.is_none());
    }

    #[test]
    fn clears_with_hysteresis() {
        let result = evaluate_low_inventory_alert(
            2_000,
            true,
            0,
            10.0,
            3_600,
            true,
            "market-a",
            "BYC",
            "xch1addr",
            None,
            100,
            111,
            true,
            Some(1_000),
        );
        assert!(!result.state.is_low);
        assert!(result.event.is_none());
    }
}
