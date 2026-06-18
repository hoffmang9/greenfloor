use std::path::Path;

const MINIMAL_PROGRAM_TEMPLATE: &str =
    include_str!("../../../tests/fixtures/data/minimal_program.yaml");
#[allow(dead_code)] // used by lib unit tests; integration fixtures include this module too
const MINIMAL_PROGRAM_SIGNER_APPEND: &str =
    include_str!("../../../tests/fixtures/data/minimal_program_signer_append.yaml");

pub struct MinimalProgramParams<'a> {
    pub home_dir: &'a Path,
    pub dexie_api_base: &'a str,
    pub log_level: Option<&'a str>,
    pub dry_run: bool,
    pub low_inventory_alerts_enabled: bool,
    pub pushover_enabled: bool,
}

impl<'a> Default for MinimalProgramParams<'a> {
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

fn materialize_minimal_program_text(params: MinimalProgramParams<'_>) -> String {
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

#[allow(dead_code)] // used by integration fixtures; lib unit tests include this module too
pub fn write_minimal_program(path: &Path, params: MinimalProgramParams<'_>) {
    std::fs::write(path, materialize_minimal_program_text(params)).expect("write minimal program");
}

#[allow(dead_code)] // used by lib unit tests; integration fixtures include this module too
pub fn write_minimal_program_with_signer(path: &Path, params: MinimalProgramParams<'_>) {
    let launcher_id = "aa".repeat(32);
    let mut contents = materialize_minimal_program_text(params);
    contents.push('\n');
    contents.push_str(
        &MINIMAL_PROGRAM_SIGNER_APPEND.replace("__LAUNCHER_ID__", &launcher_id),
    );
    std::fs::write(path, contents).expect("write signer program");
}
