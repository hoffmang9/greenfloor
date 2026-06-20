//! Shared helpers for manager CLI in-process tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::context::ManagerContext;
use super::json::ManagerOutput;
use super::runtime::{EnvReader, ManagerRuntime, PromptReader, StdioPromptReader};
use crate::error::SignerError;
use crate::error::SignerResult;
use crate::minimal_program_template::{write_minimal_program_with_signer, MinimalProgramParams};

#[path = "test_support/capturing_output.rs"]
mod capturing_output;

pub use capturing_output::TestJsonCapture;

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

#[derive(Debug, Clone, Default)]
struct MapEnvReader {
    values: HashMap<String, String>,
}

// Unmapped keys fall back to the real process environment so tests can override
// only the vars they care about (for example doctor env warnings).
impl EnvReader for MapEnvReader {
    fn var(&self, name: &str) -> String {
        self.values
            .get(name)
            .cloned()
            .unwrap_or_else(|| std::env::var(name).unwrap_or_default())
    }
}

#[derive(Clone)]
struct QueuedPromptReader {
    lines: Arc<Mutex<Vec<String>>>,
}

impl PromptReader for QueuedPromptReader {
    fn read_line(&self, _prompt: &str) -> SignerResult<String> {
        let mut guard = self.lines.lock().expect("prompt lines lock");
        guard
            .first()
            .cloned()
            .map(|line| {
                guard.remove(0);
                line
            })
            .ok_or_else(|| SignerError::Other("test prompt queue exhausted".to_string()))
    }
}

fn test_runtime(env: &[(&str, &str)], prompts: &[&str]) -> ManagerRuntime {
    let mut values = HashMap::new();
    for (name, value) in env {
        values.insert((*name).to_string(), (*value).to_string());
    }
    let prompt_lines: Vec<String> = prompts.iter().map(|line| (*line).to_string()).collect();
    let prompt: Arc<dyn PromptReader> = if prompt_lines.is_empty() {
        Arc::new(StdioPromptReader)
    } else {
        Arc::new(QueuedPromptReader {
            lines: Arc::new(Mutex::new(prompt_lines)),
        })
    };
    ManagerRuntime::from_readers(Arc::new(MapEnvReader { values }), prompt)
}

pub struct CapturedManagerContext {
    pub ctx: ManagerContext,
    pub captured: Arc<Mutex<Vec<Value>>>,
}

pub struct ManagerContextBuilder {
    program_config: PathBuf,
    markets_config: PathBuf,
    cats_config: Option<PathBuf>,
    scratch_dir: Option<PathBuf>,
    state_db: String,
    dexie_base_url: Option<String>,
    testnet_markets_path: Option<PathBuf>,
    json_compact: bool,
    env_overrides: Vec<(String, String)>,
    prompt_lines: Vec<String>,
}

impl ManagerContextBuilder {
    pub fn new(program_config: PathBuf, markets_config: PathBuf) -> Self {
        Self {
            program_config,
            markets_config,
            cats_config: None,
            scratch_dir: None,
            state_db: String::new(),
            dexie_base_url: None,
            testnet_markets_path: None,
            json_compact: true,
            env_overrides: Vec::new(),
            prompt_lines: Vec::new(),
        }
    }

    pub fn cats_config(mut self, path: PathBuf) -> Self {
        self.cats_config = Some(path);
        self
    }

    pub fn scratch_dir(mut self, path: PathBuf) -> Self {
        self.scratch_dir = Some(path);
        self
    }

    pub fn state_db(mut self, path: impl Into<String>) -> Self {
        self.state_db = path.into();
        self
    }

    pub fn testnet_markets(mut self, path: PathBuf) -> Self {
        self.testnet_markets_path = Some(path);
        self
    }

    pub fn json_compact(mut self, compact: bool) -> Self {
        self.json_compact = compact;
        self
    }

    pub fn env_overrides(mut self, values: &[(&str, &str)]) -> Self {
        self.env_overrides = values
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        self
    }

    pub fn prompt_lines(mut self, lines: &[&str]) -> Self {
        self.prompt_lines = lines.iter().map(|line| (*line).to_string()).collect();
        self
    }

    fn assemble_inner(
        self,
        output: ManagerOutput,
        json_capture: Option<TestJsonCapture>,
    ) -> ManagerContext {
        let ManagerContextBuilder {
            program_config,
            markets_config,
            cats_config,
            scratch_dir,
            state_db,
            dexie_base_url,
            testnet_markets_path,
            env_overrides,
            prompt_lines,
            ..
        } = self;
        let resolved_cats = cats_config.unwrap_or_else(|| {
            let scratch = scratch_dir.expect("cats_config or scratch_dir required");
            let path = scratch.join("unused-cats.yaml");
            if !path.exists() {
                std::fs::write(&path, "cats: []\n").expect("write unused cats");
            }
            path
        });
        let env_pairs: Vec<(&str, &str)> = env_overrides
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let prompt_refs: Vec<&str> = prompt_lines.iter().map(String::as_str).collect();
        let runtime = if env_pairs.is_empty() && prompt_refs.is_empty() {
            ManagerRuntime::production()
        } else {
            test_runtime(&env_pairs, &prompt_refs)
        };
        ManagerContext::from_test_parts(
            output,
            runtime,
            program_config,
            markets_config,
            resolved_cats,
            state_db,
            dexie_base_url,
            testnet_markets_path,
            json_capture,
        )
    }

    pub fn build(self) -> ManagerContext {
        let json_compact = self.json_compact;
        self.assemble_inner(ManagerOutput::new(json_compact), None)
    }

    pub fn build_capturing(self) -> CapturedManagerContext {
        let json_compact = self.json_compact;
        let (capture, buffer) = TestJsonCapture::new();
        let ctx = self.assemble_inner(ManagerOutput::new(json_compact), Some(capture));
        CapturedManagerContext {
            ctx,
            captured: buffer,
        }
    }
}
