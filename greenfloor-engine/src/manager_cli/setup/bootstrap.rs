use std::path::{Path, PathBuf};

use serde_json::json;

use crate::config::yaml_file::{read_yaml_file_labeled, write_yaml_file};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::paths::expand_home;
use crate::operator_log::{LogContext, HOME_BOOTSTRAP};
use crate::storage::SqliteStore;

pub struct BootstrapHomeParams<'a> {
    pub ctx: &'a ManagerContext,
    pub home_dir: &'a Path,
    pub program_template: &'a Path,
    pub markets_template: &'a Path,
    pub cats_template: Option<&'a Path>,
    pub testnet_markets_template: Option<&'a Path>,
    pub seed_testnet_markets: bool,
    pub force: bool,
}

struct HomeLayout {
    home: PathBuf,
    config_dir: PathBuf,
    db_dir: PathBuf,
    state_dir: PathBuf,
    logs_dir: PathBuf,
}

pub fn run_bootstrap_home(params: &BootstrapHomeParams<'_>) -> SignerResult<i32> {
    let BootstrapHomeParams {
        ctx,
        home_dir,
        program_template,
        markets_template,
        cats_template,
        testnet_markets_template,
        seed_testnet_markets,
        force,
    } = *params;
    let layout = prepare_home_layout(home_dir)?;
    let seeded_program = layout.config_dir.join("program.yaml");
    let seeded_markets = layout.config_dir.join("markets.yaml");
    let seeded_cats = layout.config_dir.join("cats.yaml");
    let seeded_testnet_markets = layout.config_dir.join("testnet-markets.yaml");

    let wrote_program =
        seed_program_config(&seeded_program, program_template, &layout.home, force)?;
    let wrote_markets = seed_yaml_copy(&seeded_markets, markets_template, force)?;
    let wrote_cats = seed_optional(&seeded_cats, cats_template, force)?;
    let wrote_testnet_markets = if seed_testnet_markets {
        seed_optional(&seeded_testnet_markets, testnet_markets_template, force)?
    } else {
        false
    };

    let db_path = layout.db_dir.join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path)?;
    LogContext::VALIDATION.audit(
        &store,
        HOME_BOOTSTRAP,
        &json!({
            "home_dir": layout.home.display().to_string(),
            "program_config": seeded_program.display().to_string(),
            "markets_config": seeded_markets.display().to_string(),
            "cats_config": seeded_cats.display().to_string(),
            "testnet_markets_config": seeded_testnet_markets.display().to_string(),
            "force": force,
        }),
        None,
    )?;

    ctx.emit_json(&json!({
        "bootstrapped": true,
        "home_dir": layout.home.display().to_string(),
        "program_config": seeded_program.display().to_string(),
        "markets_config": seeded_markets.display().to_string(),
        "cats_config": seeded_cats.display().to_string(),
        "testnet_markets_config": if seed_testnet_markets {
            seeded_testnet_markets.display().to_string()
        } else {
            String::new()
        },
        "state_db": db_path.display().to_string(),
        "state_dir": layout.state_dir.display().to_string(),
        "logs_dir": layout.logs_dir.display().to_string(),
        "wrote_program_config": wrote_program,
        "wrote_markets_config": wrote_markets,
        "wrote_cats_config": wrote_cats,
        "wrote_testnet_markets_config": wrote_testnet_markets,
    }))?;
    Ok(0)
}

fn prepare_home_layout(home_dir: &Path) -> SignerResult<HomeLayout> {
    let home = expand_home(home_dir);
    let layout = HomeLayout {
        config_dir: home.join("config"),
        db_dir: home.join("db"),
        state_dir: home.join("state"),
        logs_dir: home.join("logs"),
        home,
    };
    for dir in [
        &layout.home,
        &layout.config_dir,
        &layout.db_dir,
        &layout.state_dir,
        &layout.logs_dir,
    ] {
        std::fs::create_dir_all(dir).map_err(|err| {
            SignerError::Other(format!("failed to create {}: {err}", dir.display()))
        })?;
    }
    Ok(layout)
}

fn seed_optional(dest: &Path, template: Option<&Path>, force: bool) -> SignerResult<bool> {
    match template {
        Some(template) => seed_yaml_copy(dest, template, force),
        None => Ok(false),
    }
}

fn seed_yaml_copy(dest: &Path, template: &Path, force: bool) -> SignerResult<bool> {
    if !force && dest.exists() {
        return Ok(false);
    }
    let data = read_yaml_file_labeled(template, "config template")?;
    write_yaml_file(dest, &data)?;
    Ok(true)
}

fn seed_program_config(
    dest: &Path,
    template: &Path,
    home: &Path,
    force: bool,
) -> SignerResult<bool> {
    if !force && dest.exists() {
        return Ok(false);
    }
    let mut program_data = read_yaml_file_labeled(template, "program template")?;
    if let Some(app) = program_data
        .get_mut("app")
        .and_then(serde_json::Value::as_object_mut)
    {
        app.insert("home_dir".to_string(), json!(home.display().to_string()));
    }
    write_yaml_file(dest, &program_data)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::test_support::copy_bootstrap_templates;

    fn bootstrap_home_in_process(
        home_dir: &Path,
        program_template: &Path,
        markets_template: &Path,
        cats_template: &Path,
        testnet_markets_template: &Path,
        seed_testnet_markets: bool,
        force: bool,
    ) -> i32 {
        let ctx = crate::manager_cli::test_support::ManagerContextBuilder::new(
            program_template.to_path_buf(),
            markets_template.to_path_buf(),
        )
        .cats_config(cats_template.to_path_buf())
        .scratch_dir(
            program_template
                .parent()
                .expect("template parent")
                .to_path_buf(),
        )
        .json_compact(false)
        .build();
        run_bootstrap_home(&BootstrapHomeParams {
            ctx: &ctx,
            home_dir,
            program_template,
            markets_template,
            cats_template: Some(cats_template),
            testnet_markets_template: Some(testnet_markets_template),
            seed_testnet_markets,
            force,
        })
        .expect("bootstrap-home")
    }

    #[test]
    fn bootstrap_home_creates_layout_and_seed_configs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home_dir = dir.path().join(".greenfloor");
        let (program_template, markets_template, cats_template, testnet_markets_template) =
            copy_bootstrap_templates(dir.path());
        assert_eq!(
            bootstrap_home_in_process(
                &home_dir,
                &program_template,
                &markets_template,
                &cats_template,
                &testnet_markets_template,
                false,
                false,
            ),
            0
        );
        assert!(home_dir.join("config").is_dir());
        assert!(home_dir.join("db").is_dir());
        assert!(home_dir.join("state").is_dir());
        assert!(home_dir.join("logs").is_dir());
        assert!(home_dir.join("db").join("greenfloor.sqlite").is_file());
        assert!(home_dir.join("config").join("program.yaml").is_file());
        assert!(home_dir.join("config").join("markets.yaml").is_file());
        assert!(home_dir.join("config").join("cats.yaml").is_file());
    }

    #[test]
    fn bootstrap_home_without_force_keeps_existing_seeded_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home_dir = dir.path().join(".greenfloor");
        let config_dir = home_dir.join("config");
        std::fs::create_dir_all(&config_dir).expect("create config");
        std::fs::write(
            config_dir.join("program.yaml"),
            "app:\n  home_dir: \"custom-home\"\n",
        )
        .expect("write program");
        std::fs::write(config_dir.join("markets.yaml"), "markets: []\n").expect("write markets");
        std::fs::write(config_dir.join("cats.yaml"), "cats: []\n").expect("write cats");
        let (program_template, markets_template, cats_template, testnet_markets_template) =
            copy_bootstrap_templates(dir.path());
        assert_eq!(
            bootstrap_home_in_process(
                &home_dir,
                &program_template,
                &markets_template,
                &cats_template,
                &testnet_markets_template,
                false,
                false,
            ),
            0
        );
        assert_eq!(
            std::fs::read_to_string(config_dir.join("program.yaml")).expect("read program"),
            "app:\n  home_dir: \"custom-home\"\n"
        );
    }

    #[test]
    fn bootstrap_home_can_seed_optional_testnet_markets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home_dir = dir.path().join(".greenfloor");
        let (program_template, markets_template, cats_template, testnet_markets_template) =
            copy_bootstrap_templates(dir.path());
        assert_eq!(
            bootstrap_home_in_process(
                &home_dir,
                &program_template,
                &markets_template,
                &cats_template,
                &testnet_markets_template,
                true,
                false,
            ),
            0
        );
        assert!(home_dir
            .join("config")
            .join("testnet-markets.yaml")
            .is_file());
    }
}
