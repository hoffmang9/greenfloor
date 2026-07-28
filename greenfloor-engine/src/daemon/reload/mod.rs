//! Config reload marker handling for the daemon loop.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};
use tracing::Level;

use crate::daemon::coinset_ws::{CoinsetWsShared, InventoryP2Index};
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{LogContext, CONFIG_RELOADED};
use crate::storage::SqliteStore;

#[cfg(test)]
mod tests;

const RELOAD_MARKER_FILE: &str = "reload_request.json";

/// Inventory p2 rebuild outcome recorded on `config_reloaded` audit events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryP2RebuildStatus {
    Ok,
    Failed,
}

impl InventoryP2RebuildStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
        }
    }
}

/// Retryable failure while processing a reload marker (marker kept for next cycle).
enum ReloadDefer {
    MarkerUnreadable(SignerError),
    DbOpenFailed,
    AuditLookupFailed {
        reload_id: String,
        err: SignerError,
    },
    AuditInsertFailed {
        reload_id: String,
        rebuild_status: InventoryP2RebuildStatus,
        err: SignerError,
    },
}

#[must_use]
pub fn reload_marker_path(state_dir: &Path) -> PathBuf {
    state_dir.join(RELOAD_MARKER_FILE)
}

#[must_use]
pub fn reload_marker_present(state_dir: &Path) -> bool {
    reload_marker_path(state_dir).is_file()
}

/// Remove the reload marker after config reload is recorded.
///
/// # Errors
///
/// Returns an error when the marker file cannot be removed.
pub fn remove_reload_marker(state_dir: &Path) -> SignerResult<()> {
    let marker = reload_marker_path(state_dir);
    if !marker.is_file() {
        return Ok(());
    }
    std::fs::remove_file(&marker).map_err(|err| {
        SignerError::Other(format!(
            "failed to remove reload marker {}: {err}",
            marker.display()
        ))
    })
}

fn warn_remove_reload_marker(state_dir: &Path) {
    if let Err(err) = remove_reload_marker(state_dir) {
        tracing::warn!(
            marker = %reload_marker_path(state_dir).display(),
            error = %err,
            "config reload marker removal failed"
        );
    }
}

fn warn_reload_defer(marker: &Path, db_path: &Path, defer: ReloadDefer) {
    match defer {
        ReloadDefer::MarkerUnreadable(err) => {
            tracing::warn!(
                marker = %marker.display(),
                error = %err,
                "config reload marker unreadable; will retry next cycle"
            );
        }
        ReloadDefer::DbOpenFailed => {
            tracing::warn!(
                db_path = %db_path.display(),
                "config reload marker present but state DB open failed; will retry next cycle"
            );
        }
        ReloadDefer::AuditLookupFailed { reload_id, err } => {
            tracing::warn!(
                reload_id = reload_id.as_str(),
                error = %err,
                "config reload marker present but audit lookup failed; will retry next cycle"
            );
        }
        ReloadDefer::AuditInsertFailed {
            reload_id,
            rebuild_status,
            err,
        } => {
            tracing::warn!(
                reload_id = reload_id.as_str(),
                inventory_p2_rebuild = rebuild_status.as_str(),
                error = %err,
                "config reload marker present but audit insert failed; will retry next cycle"
            );
        }
    }
}

fn reload_id_from_marker(path: &Path) -> SignerResult<String> {
    let content = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!(
            "failed to read reload marker {}: {err}",
            path.display()
        ))
    })?;
    if let Some(reload_id) = serde_json::from_str::<Value>(&content)
        .ok()
        .and_then(|payload| {
            payload
                .get("reload_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
    {
        return Ok(reload_id);
    }
    let metadata = std::fs::metadata(path).map_err(|err| {
        SignerError::Other(format!(
            "failed to stat reload marker {}: {err}",
            path.display()
        ))
    })?;
    let modified_secs = metadata
        .modified()
        .ok()
        .and_then(|mtime| mtime.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs());
    Ok(format!("legacy-{modified_secs}-{}", metadata.len()))
}

/// Persist and trace a successful config reload.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn record_config_reloaded(
    store: &SqliteStore,
    source: &str,
    reload_id: &str,
    inventory_p2_rebuild: InventoryP2RebuildStatus,
) -> SignerResult<()> {
    LogContext::CONFIG.dual_audit(
        store,
        Level::INFO,
        "config reloaded",
        CONFIG_RELOADED,
        &json!({
            "source": source,
            "reload_id": reload_id,
            "inventory_p2_rebuild": inventory_p2_rebuild.as_str(),
        }),
        None,
    )
}

fn apply_inventory_p2_index(coinset: &CoinsetWsShared, index: Arc<InventoryP2Index>) {
    let p2_count = index.p2s().len();
    coinset.replace_p2_index(index);
    coinset.request_reconnect();
    tracing::info!(
        p2_count,
        "applied inventory p2 index after config reload audit; websocket reconnect requested"
    );
}

fn complete_reload_marker(
    marker: &Path,
    state_dir: &Path,
    db_path: &Path,
    coinset: &CoinsetWsShared,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) -> Result<(), ReloadDefer> {
    let reload_id =
        reload_id_from_marker(marker).map_err(ReloadDefer::MarkerUnreadable)?;
    let store = SqliteStore::open(db_path).map_err(|_| ReloadDefer::DbOpenFailed)?;
    let already_recorded = store
        .recent_audit_payload_matches(CONFIG_RELOADED, "reload_id", &reload_id, 50)
        .map_err(|err| ReloadDefer::AuditLookupFailed {
            reload_id: reload_id.clone(),
            err,
        })?;
    if already_recorded {
        warn_remove_reload_marker(state_dir);
        return Ok(());
    }

    // Build only — do not mutate live filters until the reload audit is durable.
    let (rebuild_status, pending_index) =
        match InventoryP2Index::from_markets(markets_path, testnet_markets_path) {
            Ok(index) => (InventoryP2RebuildStatus::Ok, Some(index)),
            Err(err) => {
                tracing::warn!(
                    markets_path = %markets_path.display(),
                    error = %err,
                    "inventory p2 rebuild failed during config reload; keeping prior filters"
                );
                (InventoryP2RebuildStatus::Failed, None)
            }
        };
    record_config_reloaded(&store, "reload_marker", &reload_id, rebuild_status).map_err(
        |err| ReloadDefer::AuditInsertFailed {
            reload_id,
            rebuild_status,
            err,
        },
    )?;
    if let Some(index) = pending_index {
        apply_inventory_p2_index(coinset, index);
    }
    warn_remove_reload_marker(state_dir);
    Ok(())
}

/// Best-effort reload marker handling for the daemon loop.
///
/// Builds a new inventory p2 index from markets, records `config_reloaded` (with
/// `inventory_p2_rebuild` = `ok`/`failed`), then applies the index and requests a
/// Coinset WS reconnect only after that audit is durable. A failed build keeps
/// prior filters.
pub fn handle_reload_marker_if_present(
    state_dir: &Path,
    db_path: &Path,
    coinset: &Arc<CoinsetWsShared>,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) {
    let marker = reload_marker_path(state_dir);
    if !marker.is_file() {
        return;
    }
    if let Err(defer) = complete_reload_marker(
        &marker,
        state_dir,
        db_path,
        coinset,
        markets_path,
        testnet_markets_path,
    ) {
        warn_reload_defer(&marker, db_path, defer);
    }
}
