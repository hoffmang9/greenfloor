//! Shared helpers for manager CLI in-process tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::context::ManagerContext;
use super::json::ManagerOutput;

const BOOTSTRAP_FIXTURE_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/data/bootstrap");

static RUNTIME_OVERRIDES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static PROMPT_LINES: Mutex<Option<Vec<String>>> = Mutex::new(None);

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

pub fn test_env_override(name: &str) -> Option<String> {
    RUNTIME_OVERRIDES.lock().ok()?.as_ref()?.get(name).cloned()
}

pub struct TestRuntimeOverrides {
    _private: (),
}

impl TestRuntimeOverrides {
    pub fn new(values: &[(&str, &str)]) -> Self {
        let mut map = HashMap::new();
        for (name, value) in values {
            map.insert((*name).to_string(), (*value).to_string());
        }
        *RUNTIME_OVERRIDES.lock().expect("override lock") = Some(map);
        Self { _private: () }
    }
}

impl Drop for TestRuntimeOverrides {
    fn drop(&mut self) {
        *RUNTIME_OVERRIDES.lock().expect("override lock") = None;
    }
}

pub fn take_prompt_line() -> Option<String> {
    let mut guard = PROMPT_LINES.lock().expect("prompt lines lock");
    let lines = guard.as_mut()?;
    if lines.is_empty() {
        return None;
    }
    Some(lines.remove(0))
}

pub struct TestPromptLines {
    _private: (),
}

impl TestPromptLines {
    pub fn new(lines: Vec<&str>) -> Self {
        *PROMPT_LINES.lock().expect("prompt lines lock") =
            Some(lines.into_iter().map(str::to_string).collect());
        Self { _private: () }
    }
}

impl Drop for TestPromptLines {
    fn drop(&mut self) {
        *PROMPT_LINES.lock().expect("prompt lines lock") = None;
    }
}

pub struct CapturedManagerContext {
    pub ctx: ManagerContext,
    pub captured: Arc<Mutex<Vec<Value>>>,
}

pub struct ManagerContextBuilder {
    program_config: PathBuf,
    markets_config: PathBuf,
    cats_config: Option<PathBuf>,
    state_db: String,
    dexie_base_url: Option<String>,
    testnet_markets_path: Option<PathBuf>,
    json_compact: bool,
}

impl ManagerContextBuilder {
    pub fn new(program_config: PathBuf, markets_config: PathBuf) -> Self {
        Self {
            program_config,
            markets_config,
            cats_config: None,
            state_db: String::new(),
            dexie_base_url: None,
            testnet_markets_path: None,
            json_compact: true,
        }
    }

    pub fn cats_config(mut self, path: PathBuf) -> Self {
        self.cats_config = Some(path);
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

    pub fn build(self) -> ManagerContext {
        let output = ManagerOutput::new(self.json_compact);
        ManagerContext::from_test_parts(
            output,
            self.program_config,
            self.markets_config,
            self.cats_config
                .unwrap_or_else(|| PathBuf::from("/tmp/unused-cats.yaml")),
            self.state_db,
            self.dexie_base_url,
            self.testnet_markets_path,
        )
    }

    pub fn build_capturing(self) -> CapturedManagerContext {
        let (output, captured) = ManagerOutput::capturing(self.json_compact);
        let ctx = ManagerContext::from_test_parts(
            output,
            self.program_config,
            self.markets_config,
            self.cats_config
                .unwrap_or_else(|| PathBuf::from("/tmp/unused-cats.yaml")),
            self.state_db,
            self.dexie_base_url,
            self.testnet_markets_path,
        );
        CapturedManagerContext { ctx, captured }
    }
}
