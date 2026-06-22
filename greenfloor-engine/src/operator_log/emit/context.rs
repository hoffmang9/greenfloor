use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::audit::{persist_and_mirror, persist_only, trace_payload_mirror};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AuditDurability {
    #[default]
    Required,
    BestEffort,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DualAudit<'a> {
    level: Level,
    trace_message: &'static str,
    event_type: &'a str,
    payload: &'a Value,
    market_id: Option<&'a str>,
}

impl<'a> DualAudit<'a> {
    #[must_use]
    pub(crate) fn new(
        level: Level,
        trace_message: &'static str,
        event_type: &'a str,
        payload: &'a Value,
    ) -> Self {
        Self {
            level,
            trace_message,
            event_type,
            payload,
            market_id: None,
        }
    }

    #[must_use]
    pub(crate) fn with_market_id(mut self, market_id: &'a str) -> Self {
        self.market_id = Some(market_id);
        self
    }

    pub(crate) const fn level(self) -> Level {
        self.level
    }

    pub(crate) const fn trace_message(self) -> &'static str {
        self.trace_message
    }

    pub(crate) const fn event_type(self) -> &'a str {
        self.event_type
    }

    pub(crate) const fn payload(self) -> &'a Value {
        self.payload
    }

    pub(crate) const fn market_id(self) -> Option<&'a str> {
        self.market_id
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LogContext {
    service: &'static str,
    phase: &'static str,
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

    #[must_use]
    pub const fn service(self) -> &'static str {
        self.service
    }

    #[must_use]
    pub const fn phase(self) -> &'static str {
        self.phase
    }

    fn audit_ref<'a>(
        level: Level,
        trace_message: &'static str,
        event_type: &'a str,
        payload: &'a Value,
        market_id: Option<&'a str>,
    ) -> DualAudit<'a> {
        let audit = DualAudit::new(level, trace_message, event_type, payload);
        match market_id {
            Some(market_id) => audit.with_market_id(market_id),
            None => audit,
        }
    }

    /// Persist and mirror one audit outcome.
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn dual_audit(
        self,
        store: &SqliteStore,
        level: Level,
        trace_message: &'static str,
        event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        persist_and_mirror(
            store,
            self,
            &Self::audit_ref(level, trace_message, event_type, payload, market_id),
        )
    }

    /// Mirror one audit payload to trace without persisting.
    pub fn dual_trace(
        self,
        level: Level,
        trace_message: &'static str,
        event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) {
        trace_payload_mirror(
            self,
            &Self::audit_ref(level, trace_message, event_type, payload, market_id),
        );
    }

    /// Persist an audit row without a trace mirror.
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn audit(
        self,
        store: &SqliteStore,
        event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        persist_only(
            store,
            event_type,
            payload,
            market_id,
            AuditDurability::Required,
        )
    }

    /// Persist an audit row without a trace mirror; DB errors are logged and ignored.
    pub fn audit_best_effort(
        self,
        store: &SqliteStore,
        event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) {
        let _ = persist_only(
            store,
            event_type,
            payload,
            market_id,
            AuditDurability::BestEffort,
        );
    }
}
