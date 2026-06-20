//! Injectable env and prompt boundaries for the manager CLI.

use std::io::{self, Write};
use std::sync::Arc;

use crate::error::{SignerError, SignerResult};

pub trait EnvReader: Send + Sync {
    fn var(&self, name: &str) -> String;
}

pub trait PromptReader: Send + Sync {
    fn read_line(&self, prompt: &str) -> SignerResult<String>;
}

#[derive(Debug, Clone, Default)]
pub struct OsEnvReader;

impl EnvReader for OsEnvReader {
    fn var(&self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default)]
pub struct StdioPromptReader;

impl PromptReader for StdioPromptReader {
    fn read_line(&self, prompt: &str) -> SignerResult<String> {
        eprint!("{prompt}");
        io::stderr()
            .flush()
            .map_err(|err| SignerError::Other(format!("stderr flush failed: {err}")))?;
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|err| SignerError::Other(format!("stdin read failed: {err}")))?;
        Ok(line.trim().to_string())
    }
}

#[derive(Clone)]
pub struct ManagerRuntime {
    env: Arc<dyn EnvReader>,
    prompt: Arc<dyn PromptReader>,
}

impl ManagerRuntime {
    #[must_use]
    pub fn production() -> Self {
        Self::from_readers(Arc::new(OsEnvReader), Arc::new(StdioPromptReader))
    }

    pub(crate) fn from_readers(env: Arc<dyn EnvReader>, prompt: Arc<dyn PromptReader>) -> Self {
        Self { env, prompt }
    }

    #[must_use]
    pub fn env_var(&self, name: &str) -> String {
        self.env.var(name)
    }

    pub fn prompt_line(&self, prompt: &str) -> SignerResult<String> {
        self.prompt.read_line(prompt)
    }
}

impl std::fmt::Debug for ManagerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagerRuntime").finish_non_exhaustive()
    }
}
