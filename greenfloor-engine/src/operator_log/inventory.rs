//! Operator log event inventory — tracing events and audit types for important paths.
//!
//! Each entry defines: event name, path, outcome(s), severity, correlation context, fields.
//! Audit-only paths are documented in [`audit_only`] — no duplicate tracing event.

/// Tracing event emitted when a daemon cycle begins.
pub const DAEMON_CYCLE_STARTED: &str = "daemon_cycle_started";
/// Tracing event when a daemon cycle finishes (success or partial failure).
pub const DAEMON_CYCLE_COMPLETED: &str = "daemon_cycle_completed";
/// Tracing event when a market post-reconcile phase batch begins.
pub const MARKET_CYCLE_STARTED: &str = "market_cycle_started";
/// Tracing event when a market post-reconcile phase batch completes.
pub const MARKET_CYCLE_COMPLETED: &str = "market_cycle_completed";
/// Tracing event at each market phase boundary (`inventory` / `strategy` / `cancel` / `coin_ops`).
pub const MARKET_PHASE: &str = "market_phase";
/// Tracing event for manager `build-and-post-offer` command completion.
pub const OFFER_POST_COMPLETED: &str = "offer_post_completed";
/// Tracing event for a single build-and-post iteration.
pub const OFFER_POST_ITERATION: &str = "offer_post_iteration";
/// Tracing event when daemon config reload marker is consumed.
pub const CONFIG_RELOADED: &str = "config_reloaded";
/// Tracing event for markets validation warnings (non-fatal).
pub const MARKET_VALIDATION_WARNING: &str = "market_validation_warning";

// Documented paths that intentionally omit tracing (audit or JSON stdout covers them).
pub mod audit_only {
    // Coinset websocket connect/disconnect/mempool/tx_block/coin watch — `coinset_ws_*`.
    pub const COINSET_WEBSOCKET: &str = "daemon/coinset_ws/*";
    // Offer lifecycle reconcile transitions — `offer_lifecycle_transition`.
    pub const RECONCILE_TRANSITIONS: &str = "offer/lifecycle/persist.rs";
    // Daemon strategy/coin-op/inventory audit chain — `strategy_*`, `coin_ops_*`, `inventory_*`.
    pub const DAEMON_MARKET_PHASES: &str = "daemon/*_phase.rs";
    // Cycle preamble price/mempool polls — `xch_price_*`, `coinset_mempool_*`.
    pub const CYCLE_PREAMBLE: &str = "daemon/preamble.rs";
}

// Paths not yet wired for operator logging (documented gap).
pub mod not_wired {
    // Low-inventory alert evaluation — logic in `cycle/notifications.rs`; no daemon hook or
    // notification sink. Will log `low_inventory_alert` audit+trace when wired to inventory phase.
    pub const LOW_INVENTORY_ALERTS: &str = "cycle/notifications.rs";
    // Manager coin-op CLI — JSON stdout only; audit deferred until persist path exists.
    pub const MANAGER_COIN_OPS_CLI: &str = "manager_cli/coin_ops/*";
}

/// Inventory row for test verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryEntry {
    pub event: &'static str,
    pub path: &'static str,
    pub outcomes: &'static [&'static str],
    pub severity: &'static str,
    pub correlation: &'static str,
    pub fields: &'static [&'static str],
}

static TRACING_INVENTORY: [InventoryEntry; 9] = [
    InventoryEntry {
        event: DAEMON_CYCLE_STARTED,
        path: "daemon/cycle_entry.rs",
        outcomes: &["started"],
        severity: "INFO",
        correlation: "cycle",
        fields: &[
            "service",
            "event",
            "phase",
            "market_count",
            "dry_run",
            "selected_market_ids",
        ],
    },
    InventoryEntry {
        event: DAEMON_CYCLE_COMPLETED,
        path: "daemon/cycle_entry.rs",
        outcomes: &["success", "partial_failure"],
        severity: "INFO|WARN",
        correlation: "cycle",
        fields: &[
            "service",
            "event",
            "phase",
            "exit_code",
            "cycle_error_count",
            "elapsed_ms",
            "market_count",
        ],
    },
    InventoryEntry {
        event: MARKET_CYCLE_STARTED,
        path: "daemon/market_cycle.rs",
        outcomes: &["started"],
        severity: "DEBUG",
        correlation: "cycle,market_id",
        fields: &["service", "event", "phase", "market_id", "outcome"],
    },
    InventoryEntry {
        event: MARKET_CYCLE_COMPLETED,
        path: "daemon/market_cycle.rs",
        outcomes: &["success", "failure"],
        severity: "DEBUG|WARN",
        correlation: "cycle,market_id",
        fields: &[
            "service",
            "event",
            "phase",
            "market_id",
            "outcome",
            "cycle_errors",
        ],
    },
    InventoryEntry {
        event: MARKET_PHASE,
        path: "daemon/market_cycle.rs",
        outcomes: &["started", "completed", "failed"],
        severity: "DEBUG|WARN",
        correlation: "cycle,market_id,phase",
        fields: &["service", "event", "phase", "market_id", "outcome"],
    },
    InventoryEntry {
        event: OFFER_POST_COMPLETED,
        path: "offer/operator/build_and_post/mod.rs",
        outcomes: &["success", "partial_failure", "failure"],
        severity: "INFO|WARN|ERROR",
        correlation: "market_id",
        fields: &[
            "service",
            "event",
            "phase",
            "market_id",
            "outcome",
            "publish_attempts",
            "publish_failures",
            "dry_run",
        ],
    },
    InventoryEntry {
        event: OFFER_POST_ITERATION,
        path: "offer/operator/build_and_post/iteration.rs",
        outcomes: &["success", "failure", "preview"],
        severity: "INFO|WARN",
        correlation: "market_id",
        fields: &[
            "service",
            "event",
            "phase",
            "market_id",
            "outcome",
            "publish_venue",
            "error",
            "offer_ref",
        ],
    },
    InventoryEntry {
        event: CONFIG_RELOADED,
        path: "daemon/reload.rs",
        outcomes: &["success"],
        severity: "INFO",
        correlation: "daemon_loop",
        fields: &["service", "event", "phase", "source"],
    },
    InventoryEntry {
        event: MARKET_VALIDATION_WARNING,
        path: "config/markets_validate.rs",
        outcomes: &["warning"],
        severity: "WARN",
        correlation: "market_id",
        fields: &["service", "event", "market_id", "field", "value"],
    },
];

/// Canonical inventory of tracing events this module owns.
#[must_use]
pub fn tracing_inventory() -> &'static [InventoryEntry] {
    &TRACING_INVENTORY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_inventory_covers_all_declared_events() {
        let events: Vec<_> = tracing_inventory()
            .iter()
            .map(|entry| entry.event)
            .collect();
        assert!(events.contains(&DAEMON_CYCLE_STARTED));
        assert!(events.contains(&OFFER_POST_ITERATION));
        assert_eq!(events.len(), tracing_inventory().len());
    }
}
