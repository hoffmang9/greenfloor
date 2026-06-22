use serde_json::json;

use crate::config::read_program_yaml;
use crate::config::yaml_file::write_yaml_file;
use crate::error::{SignerError, SignerResult};
use crate::file_logging::validate_log_level;
use crate::manager_cli::context::ManagerContext;

pub fn run_set_log_level(ctx: &ManagerContext, log_level: &str) -> SignerResult<i32> {
    let program_path = &ctx.program_config;
    let level = validate_log_level(log_level)?;
    let mut root = read_program_yaml(program_path)?;
    let app = root
        .as_object_mut()
        .ok_or_else(|| SignerError::Other("program config root must be a mapping".to_string()))?;
    let app_entry = app.entry("app".to_string()).or_insert_with(|| json!({}));
    let app_map = app_entry.as_object_mut().ok_or_else(|| {
        SignerError::Other("program config field 'app' must be a mapping".to_string())
    })?;
    let prior_level = app_map
        .get("log_level")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| "INFO".to_string(), str::to_ascii_uppercase);
    app_map.insert("log_level".to_string(), json!(level));
    write_yaml_file(program_path, &root)?;
    ctx.emit_json(&json!({
        "updated": true,
        "program_config": program_path.display().to_string(),
        "previous_log_level": prior_level,
        "log_level": level,
    }))?;
    Ok(0)
}
