use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::audit::{mirror_only, persist_and_mirror, persist_only};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditDurability {
    #[default]
    Required,
    BestEffort,
}

#[derive(Debug, Clone, Copy)]
pub struct LogContext {
    pub service: &'static str,
    pub phase: &'static str,
}

impl LogContext {
    pub const DAEMON_CYCLE: Self = Self {
        service: "daemon",
        phase: "daemon_cycle",
    };
    pub const MARKET_CYCLE: Self = Self {
        service: "daemon",
        phase: "market_cycle",
    };
    pub const OFFER_POST: Self = Self {
        service: "manager",
        phase: "offer_post",
    };
    pub const CONFIG: Self = Self {
        service: "daemon",
        phase: "config",
    };
    pub const VALIDATION: Self = Self {
        service: "manager",
        phase: "validation",
    };
    pub const COINSET: Self = Self {
        service: "daemon",
        phase: "coinset",
    };

    /// Persist and mirror one audit outcome (`AuditDurability::Required`).
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn dual_audit(
        self,
        store: &SqliteStore,
        level: Level,
        trace_message: &'static str,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        persist_and_mirror(
            store,
            self,
            level,
            trace_message,
            audit_event_type,
            payload,
            market_id,
        )
    }

    /// Mirror one audit outcome to trace without persisting.
    ///
    /// # Errors
    ///
    /// Always returns `Ok(())` today; reserved for future trace-side failures.
    pub fn dual_trace(
        self,
        level: Level,
        trace_message: &'static str,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        mirror_only(
            self,
            level,
            trace_message,
            audit_event_type,
            payload,
            market_id,
        );
        Ok(())
    }

    /// Persist an audit row without a trace mirror (`AuditDurability::Required`).
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn audit(
        self,
        store: &SqliteStore,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        self.audit_with(
            store,
            audit_event_type,
            payload,
            market_id,
            AuditDurability::Required,
        )
    }

    /// Persist an audit row without a trace mirror.
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn audit_with(
        self,
        store: &SqliteStore,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
        durability: AuditDurability,
    ) -> SignerResult<()> {
        persist_only(store, audit_event_type, payload, market_id, durability)
    }
}
