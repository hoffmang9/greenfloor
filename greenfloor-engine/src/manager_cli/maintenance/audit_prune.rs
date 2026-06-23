use crate::config::load_program_config;
use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::storage::{resolve_state_db_path, PruneAuditEventsOptions, SqliteStore};

pub fn run_audit_prune(ctx: &ManagerContext, dry_run: bool, vacuum: bool) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
    let store = SqliteStore::open(&db_path)?;
    let report = store.prune_stale_audit_events(
        program.storage_audit_retention_days,
        PruneAuditEventsOptions::cli(dry_run, vacuum),
    )?;
    ctx.emit_json(&serde_json::json!({
        "state_db": db_path.display().to_string(),
        "dry_run": report.dry_run,
        "retention_days": report.retention_days,
        "cutoff": report.cutoff.to_rfc3339(),
        "deletable_count": report.deletable_count,
        "deleted_count": report.deleted_count,
        "vacuum_ran": report.vacuum_ran,
    }))?;
    Ok(0)
}
