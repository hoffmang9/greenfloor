//! Shared manager CLI session (output mode + resolved config paths).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::cli_util::optional_str;
use crate::error::SignerResult;

use super::commands::{ManagerCli, ManagerCommands};
use super::json::ManagerOutput;
use super::paths::{
    default_cats_config_path, default_markets_config_path, default_program_config_path,
    default_testnet_markets_config_path, resolve_cli_config_path,
};
use super::runtime::ManagerRuntime;

#[derive(Debug, Clone)]
pub struct ManagerContext {
    pub output: ManagerOutput,
    pub runtime: ManagerRuntime,
    pub program_config: PathBuf,
    pub markets_config: PathBuf,
    pub cats_config: PathBuf,
    pub state_db: String,
    pub dexie_base_url: Option<String>,
    testnet_markets_path: Option<PathBuf>,
}

impl ManagerContext {
    pub fn from_cli(cli: ManagerCli) -> (Self, ManagerCommands) {
        let ManagerCli {
            program_config,
            markets_config,
            testnet_markets_config,
            cats_config,
            state_db,
            json,
            dexie_base_url,
            command,
        } = cli;
        let testnet_markets_path = resolve_testnet_markets_path(&testnet_markets_config);
        (
            Self {
                output: ManagerOutput::new(json),
                runtime: ManagerRuntime::production(),
                program_config: resolve_cli_config_path(
                    &program_config,
                    Path::new("config/program.yaml"),
                    default_program_config_path,
                ),
                markets_config: resolve_cli_config_path(
                    &markets_config,
                    Path::new("config/markets.yaml"),
                    default_markets_config_path,
                ),
                cats_config: resolve_cli_config_path(
                    &cats_config,
                    Path::new("config/cats.yaml"),
                    default_cats_config_path,
                ),
                state_db,
                dexie_base_url,
                testnet_markets_path,
            },
            command,
        )
    }

    pub fn testnet_markets_path(&self) -> Option<&Path> {
        self.testnet_markets_path.as_deref()
    }

    pub fn state_db_override(&self) -> Option<&str> {
        optional_str(&self.state_db)
    }

    #[must_use]
    pub fn env_var(&self, name: &str) -> String {
        self.runtime.env_var(name)
    }

    pub fn prompt_line(&self, prompt: &str) -> SignerResult<String> {
        self.runtime.prompt_line(prompt)
    }

    pub fn emit_json(&self, value: &Value) -> SignerResult<()> {
        self.output.emit_json(value)
    }

    pub fn emit_serialized<T: Serialize>(&self, value: &T) -> SignerResult<()> {
        self.output.emit_serialized(value)
    }
}

fn resolve_testnet_markets_path(testnet_markets_config: &str) -> Option<PathBuf> {
    let explicit = testnet_markets_config.trim();
    if !explicit.is_empty() {
        return Some(PathBuf::from(explicit));
    }
    default_testnet_markets_config_path()
}

#[cfg(test)]
pub mod test_builder {
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
}
