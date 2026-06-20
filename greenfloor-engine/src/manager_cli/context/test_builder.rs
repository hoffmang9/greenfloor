use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::{ManagerContext, ManagerOutput};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::runtime::{EnvReader, ManagerRuntime, PromptReader, StdioPromptReader};

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
            .inspect(|_| {
                guard.remove(0);
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

    fn assemble_inner(self, output: ManagerOutput) -> ManagerContext {
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
        ManagerContext {
            output,
            runtime,
            program_config,
            markets_config,
            cats_config: resolved_cats,
            state_db,
            dexie_base_url,
            testnet_markets_path,
        }
    }

    pub fn build(self) -> ManagerContext {
        let json_compact = self.json_compact;
        self.assemble_inner(ManagerOutput::new(json_compact))
    }

    pub fn build_capturing(self) -> CapturedManagerContext {
        let json_compact = self.json_compact;
        let (output, buffer) = ManagerOutput::capturing(json_compact);
        let ctx = self.assemble_inner(output);
        CapturedManagerContext {
            ctx,
            captured: buffer,
        }
    }
}
