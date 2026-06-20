//! Backoff policy for bounded Coinset websocket-once capture.

use std::time::Duration;

use crate::config::ManagerProgramConfig;

/// Capture window and reconnect delay for [`super::capture_coinset_websocket_once_with_timings`].
///
/// Production callers should use [`Self::from_program`]. Unit tests that exercise the once
/// capture loop should pass [`Self::UNIT_TEST`] explicitly.
#[derive(Debug, Clone, Copy)]
pub struct OnceCaptureTimings {
    pub capture_window: Duration,
    pub reconnect: Duration,
}

impl OnceCaptureTimings {
    #[must_use]
    pub fn from_program(program: &ManagerProgramConfig) -> Self {
        Self {
            capture_window: Duration::from_secs(
                program.tx_block_fallback_poll_interval_seconds.max(1),
            ),
            reconnect: Duration::from_secs(
                program.tx_block_websocket_reconnect_interval_seconds.max(1),
            ),
        }
    }

    /// Fast bounded capture for unit tests (does not depend on program interval fields).
    #[allow(dead_code)]
    pub const UNIT_TEST: Self = Self {
        capture_window: Duration::from_millis(10),
        reconnect: Duration::from_millis(1),
    };
}
