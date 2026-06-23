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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::test_support::{
        pop_json, write_program_with_signer, ManagerContextBuilder,
    };

    #[test]
    fn run_set_log_level_updates_program_yaml_and_emits_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        write_program_with_signer(&program, dir.path());
        let harness =
            ManagerContextBuilder::new(program.clone(), dir.path().join("unused-markets.yaml"))
                .scratch_dir(dir.path().to_path_buf())
                .build_capturing();
        let code = run_set_log_level(&harness.ctx, "debug").expect("set log level");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        assert_eq!(payload.get("updated"), Some(&json!(true)));
        assert_eq!(payload.get("log_level"), Some(&json!("DEBUG")));
        assert_eq!(payload.get("previous_log_level"), Some(&json!("INFO")));

        let updated = read_program_yaml(&program).expect("read program");
        assert_eq!(
            updated
                .get("app")
                .and_then(|app| app.get("log_level"))
                .and_then(serde_json::Value::as_str),
            Some("DEBUG")
        );
    }

    #[test]
    fn run_set_log_level_rejects_invalid_level() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        write_program_with_signer(&program, dir.path());
        let ctx = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
            .scratch_dir(dir.path().to_path_buf())
            .build();
        let err = run_set_log_level(&ctx, "verbose").expect_err("invalid level");
        assert!(err.to_string().contains("log level must be one of"));
    }
}
