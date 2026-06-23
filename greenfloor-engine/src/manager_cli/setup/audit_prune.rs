use chrono::{Duration, Utc};
use serde_json::json;

use crate::config::load_program_config;
use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::storage::resolve_state_db_path;
use crate::storage::SqliteStore;

pub fn run_audit_prune(ctx: &ManagerContext, dry_run: bool) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
    let store = SqliteStore::open(&db_path)?;
    let retention_days = program.storage_audit_retention_days;
    let cutoff = Utc::now() - Duration::days(i64::try_from(retention_days).unwrap_or(i64::MAX));

    if dry_run {
        let deletable = store.count_prunable_audit_events_older_than(cutoff)?;
        ctx.emit_json(&json!({
            "state_db": db_path.display().to_string(),
            "dry_run": true,
            "retention_days": retention_days,
            "cutoff": cutoff.to_rfc3339(),
            "deletable_count": deletable,
        }))?;
        return Ok(0);
    }

    let deleted = store.prune_audit_events_older_than(cutoff)?;
    ctx.emit_json(&json!({
        "state_db": db_path.display().to_string(),
        "dry_run": false,
        "retention_days": retention_days,
        "cutoff": cutoff.to_rfc3339(),
        "deleted_count": deleted,
    }))?;
    Ok(0)
}
