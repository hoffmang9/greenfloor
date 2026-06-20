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
    pub(crate) testnet_markets_path: Option<PathBuf>,
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
