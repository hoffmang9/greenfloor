use crate::config::ManagerProgramConfig;

pub(super) fn sample_program(parallelism_enabled: bool, dry_run: bool) -> ManagerProgramConfig {
    ManagerProgramConfig {
        runtime_market_slot_count: 1,
        runtime_offer_parallelism_enabled: parallelism_enabled,
        runtime_offer_parallelism_max_workers: 2,
        runtime_dry_run: dry_run,
        tx_block_websocket_reconnect_interval_seconds: 1,
        tx_block_fallback_poll_interval_seconds: 1,
        ..Default::default()
    }
}
