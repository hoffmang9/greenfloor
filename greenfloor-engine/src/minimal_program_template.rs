//! Shared minimal ``program.yaml`` template materialization for tests and script adapters.

use std::path::Path;

const MINIMAL_PROGRAM_TEMPLATE: &str =
    include_str!("../../tests/fixtures/data/minimal_program.yaml");
const MINIMAL_PROGRAM_SIGNER_APPEND: &str =
    include_str!("../../tests/fixtures/data/minimal_program_signer_append.yaml");

#[derive(Clone, Copy)]
pub struct MinimalProgramParams<'a> {
    pub home_dir: &'a Path,
    pub dexie_api_base: &'a str,
    pub log_level: Option<&'a str>,
    pub dry_run: bool,
    pub low_inventory_alerts_enabled: bool,
    pub pushover_enabled: bool,
}

impl Default for MinimalProgramParams<'_> {
    fn default() -> Self {
        Self {
            home_dir: Path::new("/tmp/greenfloor-test-home"),
            dexie_api_base: "https://api.dexie.space",
            log_level: Some("INFO"),
            dry_run: false,
            low_inventory_alerts_enabled: false,
            pushover_enabled: false,
        }
    }
}

#[must_use]
pub fn materialize_minimal_program_text(params: MinimalProgramParams<'_>) -> String {
    let log_level = params
        .log_level
        .unwrap_or("INFO")
        .trim()
        .to_ascii_uppercase();
    MINIMAL_PROGRAM_TEMPLATE
        .replace("__HOME_DIR__", &params.home_dir.display().to_string())
        .replace("__DEXIE_API_BASE__", params.dexie_api_base)
        .replace("__LOG_LEVEL__", &log_level)
        .replace("__DRY_RUN__", if params.dry_run { "true" } else { "false" })
        .replace(
            "__ALERTS_ENABLED__",
            if params.low_inventory_alerts_enabled {
                "true"
            } else {
                "false"
            },
        )
        .replace(
            "__PUSHOVER_ENABLED__",
            if params.pushover_enabled {
                "true"
            } else {
                "false"
            },
        )
}

pub fn write_minimal_program(path: &Path, params: MinimalProgramParams<'_>) {
    std::fs::write(path, materialize_minimal_program_text(params))
        .unwrap_or_else(|err| panic!("write minimal program yaml {}: {err}", path.display()));
}

pub fn write_minimal_program_with_signer(path: &Path, params: MinimalProgramParams<'_>) {
    let launcher_id = "aa".repeat(32);
    let mut contents = materialize_minimal_program_text(params);
    contents.push('\n');
    contents.push_str(&MINIMAL_PROGRAM_SIGNER_APPEND.replace("__LAUNCHER_ID__", &launcher_id));
    std::fs::write(path, contents)
        .unwrap_or_else(|err| panic!("write signer program {}: {err}", path.display()));
}
