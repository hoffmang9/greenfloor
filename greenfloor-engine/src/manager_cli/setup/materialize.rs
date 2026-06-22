use std::path::Path;

use crate::minimal_program_template::{
    write_minimal_program, write_minimal_program_with_signer, MinimalProgramParams,
};

#[derive(Clone, Copy)]
pub struct MaterializeMinimalProgramFeatureFlags {
    pub dry_run: bool,
    pub low_inventory_alerts_enabled: bool,
    pub pushover_enabled: bool,
}

#[derive(Clone, Copy)]
pub struct MaterializeMinimalProgramRequest<'a> {
    pub output: &'a Path,
    pub home_dir: &'a Path,
    pub dexie_api_base: &'a str,
    pub log_level: &'a str,
    pub features: MaterializeMinimalProgramFeatureFlags,
    pub with_signer: bool,
}

pub fn run_materialize_minimal_program(request: MaterializeMinimalProgramRequest<'_>) -> i32 {
    let params = MinimalProgramParams {
        home_dir: request.home_dir,
        dexie_api_base: request.dexie_api_base,
        log_level: Some(request.log_level),
        dry_run: request.features.dry_run,
        low_inventory_alerts_enabled: request.features.low_inventory_alerts_enabled,
        pushover_enabled: request.features.pushover_enabled,
        coinset_msp_base_url: None,
    };
    if request.with_signer {
        write_minimal_program_with_signer(request.output, params);
    } else {
        write_minimal_program(request.output, params);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialize_minimal_program_template_writes_expected_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let program = dir.path().join("program.yaml");
        let code = run_materialize_minimal_program(MaterializeMinimalProgramRequest {
            output: &program,
            home_dir: &home,
            dexie_api_base: "https://dexie.test",
            log_level: "INFO",
            features: MaterializeMinimalProgramFeatureFlags {
                dry_run: false,
                low_inventory_alerts_enabled: true,
                pushover_enabled: true,
            },
            with_signer: false,
        });
        assert_eq!(code, 0);
        let raw: serde_json::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&program).expect("read program"))
                .expect("parse yaml");
        assert_eq!(
            raw.get("app")
                .and_then(|app| app.get("home_dir"))
                .and_then(serde_json::Value::as_str),
            Some(home.to_str().expect("home path"))
        );
        assert_eq!(
            raw.get("venues")
                .and_then(|venues| venues.get("dexie"))
                .and_then(|dexie| dexie.get("api_base"))
                .and_then(serde_json::Value::as_str),
            Some("https://dexie.test")
        );
        assert_eq!(
            raw.get("dev")
                .and_then(|dev| dev.get("python"))
                .and_then(|python| python.get("min_version"))
                .and_then(serde_json::Value::as_str),
            Some("3.11")
        );
    }
}
