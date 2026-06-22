use crate::config::{load_markets_config_with_overlay, load_program_config};
use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;

pub fn validate_config(ctx: &ManagerContext, program_only: bool) -> SignerResult<()> {
    let _program = load_program_config(&ctx.program_config)?;
    if program_only {
        return Ok(());
    }
    let _markets =
        load_markets_config_with_overlay(&ctx.markets_config, ctx.testnet_markets_path())?;
    Ok(())
}

pub fn run_config_validate(ctx: &ManagerContext, program_only: bool) -> SignerResult<i32> {
    validate_config(ctx, program_only)?;
    let program_path = ctx.program_config.display().to_string();
    if program_only {
        ctx.emit_json(&serde_json::json!({
            "ok": true,
            "program_config": program_path,
        }))?;
        return Ok(0);
    }
    ctx.emit_json(&serde_json::json!({
        "ok": true,
        "program_config": program_path,
        "markets_config": ctx.markets_config.display().to_string(),
    }))?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::test_support::{
        copy_fixture_data, pop_json, repo_root, ManagerContextBuilder,
    };

    #[test]
    fn config_validate_emits_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program_path = copy_fixture_data("config_validate_program.yaml", dir.path());
        let markets_path = dir.path().join("markets.yaml");
        std::fs::write(&markets_path, "markets: []\n").expect("write markets");
        let ctx = ManagerContextBuilder::new(program_path, markets_path)
            .scratch_dir(dir.path().to_path_buf())
            .json_compact(false)
            .build();
        let code = run_config_validate(&ctx, false).expect("validate");
        assert_eq!(code, 0);
    }

    #[test]
    fn config_validate_accepts_example_configs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        std::fs::copy(repo_root().join("config/markets.yaml"), &markets).expect("copy markets");
        let harness = ManagerContextBuilder::new(program, markets)
            .cats_config(dir.path().join("unused-cats.yaml"))
            .build_capturing();
        let code = run_config_validate(&harness.ctx, false).expect("config-validate");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        assert_eq!(
            payload.get("ok").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn config_validate_program_only_accepts_example_program() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        let ctx = ManagerContextBuilder::new(program, dir.path().join("unused-markets.yaml"))
            .scratch_dir(dir.path().to_path_buf())
            .json_compact(false)
            .build();
        let code = run_config_validate(&ctx, true).expect("config-validate program-only");
        assert_eq!(code, 0);
    }
}
