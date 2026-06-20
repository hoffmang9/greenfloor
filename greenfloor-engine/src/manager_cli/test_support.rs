//! Shared helpers for manager CLI in-process tests.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;

pub use super::context::test_builder::{CapturedManagerContext, ManagerContextBuilder};
use crate::minimal_program_template::{write_minimal_program_with_signer, MinimalProgramParams};

const BOOTSTRAP_FIXTURE_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/data/bootstrap");
const FIXTURE_DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/data");

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

pub fn copy_example_program_and_markets(dir: &Path) -> (PathBuf, PathBuf) {
    let program = dir.join("program.yaml");
    let markets = dir.join("markets.yaml");
    std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
    std::fs::copy(repo_root().join("config/markets.yaml"), &markets).expect("copy markets");
    (program, markets)
}

pub fn copy_example_program(dest: &Path) -> PathBuf {
    let program = dest.join("program.yaml");
    std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
    program
}

pub fn copy_fixture_data(name: &str, dest: &Path) -> PathBuf {
    let path = dest.join(name);
    std::fs::copy(Path::new(FIXTURE_DATA_DIR).join(name), &path)
        .unwrap_or_else(|err| panic!("copy fixture {name}: {err}"));
    path
}

pub fn write_program_with_signer(path: &Path, home_dir: &Path) {
    write_minimal_program_with_signer(
        path,
        MinimalProgramParams {
            home_dir,
            ..Default::default()
        },
    );
}

pub fn write_combine_test_configs(dir: &Path, cat_asset_id: &str, with_signer: bool) {
    let program_path = dir.join("program.yaml");
    if with_signer {
        write_program_with_signer(&program_path, dir);
    } else {
        crate::minimal_program_template::write_minimal_program(
            &program_path,
            MinimalProgramParams {
                home_dir: dir,
                ..Default::default()
            },
        );
    }
    write_combine_dust_markets(
        &dir.join("markets.yaml"),
        cat_asset_id,
        COMBINE_RECEIVE_ADDRESS,
    );
    write_combine_dust_cats(&dir.join("cats.yaml"), cat_asset_id);
}

pub fn write_combine_dust_cats(path: &Path, cat_asset_id: &str) {
    let template = include_str!("../../tests/fixtures/data/combine_dust_cats.template.yaml");
    let yaml = template.replace("__CAT_ASSET_ID__", cat_asset_id);
    std::fs::write(path, yaml).expect("write combine dust cats");
}

const COMBINE_RECEIVE_ADDRESS: &str =
    "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

pub fn write_combine_dust_markets(path: &Path, cat_asset_id: &str, receive_address: &str) {
    let template = include_str!("../../tests/fixtures/data/combine_dust_markets.template.yaml");
    let yaml = template
        .replace("__CAT_ASSET_ID__", cat_asset_id)
        .replace("__RECEIVE_ADDRESS__", receive_address);
    std::fs::write(path, yaml).expect("write combine dust markets");
}

pub fn copy_bootstrap_templates(dest: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let src_dir = Path::new(BOOTSTRAP_FIXTURE_DIR);
    let program_template = dest.join("program.template.yaml");
    let markets_template = dest.join("markets.template.yaml");
    let cats_template = dest.join("cats.template.yaml");
    let testnet_markets_template = dest.join("testnet-markets.template.yaml");
    std::fs::copy(src_dir.join("program.template.yaml"), &program_template)
        .expect("copy program template");
    std::fs::copy(src_dir.join("markets.template.yaml"), &markets_template)
        .expect("copy markets template");
    std::fs::copy(src_dir.join("cats.template.yaml"), &cats_template).expect("copy cats template");
    std::fs::copy(
        src_dir.join("testnet-markets.template.yaml"),
        &testnet_markets_template,
    )
    .expect("copy testnet markets template");
    (
        program_template,
        markets_template,
        cats_template,
        testnet_markets_template,
    )
}

pub fn pop_json(captured: &Arc<Mutex<Vec<Value>>>) -> Value {
    captured
        .lock()
        .expect("capture lock")
        .pop()
        .expect("json emitted")
}
