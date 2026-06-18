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

#[derive(Debug, Clone, PartialEq)]
pub struct LowInventoryInput {
    pub now_unix: i64,
    pub low_inventory_enabled: bool,
    pub program_default_threshold: i64,
    pub clear_hysteresis_percent: f64,
    pub dedup_cooldown_seconds: i64,
    pub market_enabled: bool,
    pub market_id: String,
    pub ticker: String,
    pub receive_address: String,
    pub market_threshold: Option<i64>,
    pub low_watermark: i64,
    pub remaining: i64,
    pub state_is_low: bool,
    pub state_last_alert_at_unix: Option<i64>,
}

fn compute_low_inventory_threshold(
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

pub fn evaluate_low_inventory_alert(input: &LowInventoryInput) -> LowInventoryEvaluation {
    if !input.market_enabled || !input.low_inventory_enabled {
        return LowInventoryEvaluation {
            state: AlertState {
                is_low: input.state_is_low,
                last_alert_at_unix: input.state_last_alert_at_unix,
            },
            event: None,
        };
    }

    let threshold = compute_low_inventory_threshold(
        input.market_threshold,
        input.program_default_threshold,
        input.low_watermark,
    );
    let hysteresis_target = crate::offer::pricing::f64_to_i64_round(
        crate::offer::pricing::i64_to_f64(threshold)
            * (1.0 + input.clear_hysteresis_percent / 100.0),
    )
    .unwrap_or(threshold);

    let mut next_state = AlertState {
        is_low: input.state_is_low,
        last_alert_at_unix: input.state_last_alert_at_unix,
    };

    if input.remaining >= hysteresis_target {
        next_state.is_low = false;
        return LowInventoryEvaluation {
            state: next_state,
            event: None,
        };
    }

    if input.remaining >= threshold {
        return LowInventoryEvaluation {
            state: next_state,
            event: None,
        };
    }

    let mut should_send = !input.state_is_low || input.state_last_alert_at_unix.is_none();
    let mut reason = "low_triggered".to_string();
    if input.state_is_low
        && input.state_last_alert_at_unix.is_some()
        && input
            .now_unix
            .saturating_sub(input.state_last_alert_at_unix.unwrap_or(input.now_unix))
            >= input.dedup_cooldown_seconds
    {
        should_send = true;
        reason = "reminder_sent".to_string();
    }

    next_state.is_low = true;
    if should_send {
        next_state.last_alert_at_unix = Some(input.now_unix);
        return LowInventoryEvaluation {
            state: next_state,
            event: Some(AlertEvent {
                market_id: input.market_id.clone(),
                ticker: input.ticker.clone(),
                remaining_amount: input.remaining,
                receive_address: input.receive_address.clone(),
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
        evaluate_low_inventory_alert, AlertEvent, AlertState, LowInventoryEvaluation,
        LowInventoryInput,
    };

    fn sample_input(
        now_unix: i64,
        remaining: i64,
        state_is_low: bool,
        state_last_alert_at_unix: Option<i64>,
    ) -> LowInventoryInput {
        LowInventoryInput {
            now_unix,
            low_inventory_enabled: true,
            program_default_threshold: 0,
            clear_hysteresis_percent: 10.0,
            dedup_cooldown_seconds: 3_600,
            market_enabled: true,
            market_id: "market-a".to_string(),
            ticker: "BYC".to_string(),
            receive_address: "xch1addr".to_string(),
            market_threshold: None,
            low_watermark: 100,
            remaining,
            state_is_low,
            state_last_alert_at_unix,
        }
    }

    #[test]
    fn first_low_inventory_alert_fires() {
        let result = evaluate_low_inventory_alert(&sample_input(1_000, 90, false, None));
        assert_eq!(
            result,
            LowInventoryEvaluation {
                state: AlertState {
                    is_low: true,
                    last_alert_at_unix: Some(1_000),
                },
                event: Some(AlertEvent {
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
        let result = evaluate_low_inventory_alert(&sample_input(2_000, 80, true, Some(1_000)));
        assert!(result.state.is_low);
        assert!(result.event.is_none());
    }

    #[test]
    fn clears_with_hysteresis() {
        let result = evaluate_low_inventory_alert(&sample_input(2_000, 111, true, Some(1_000)));
        assert!(!result.state.is_low);
        assert!(result.event.is_none());
    }
}
