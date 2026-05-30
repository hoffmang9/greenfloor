use std::path::{Path, PathBuf};

/// Config file paths shared by daemon cycle phases and offer dispatch.
#[derive(Debug, Clone)]
pub struct DaemonConfigPaths {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
}

impl DaemonConfigPaths {
    pub fn new(
        program_path: PathBuf,
        markets_path: PathBuf,
        testnet_markets_path: Option<PathBuf>,
    ) -> Self {
        Self {
            program_path,
            markets_path,
            testnet_markets_path,
        }
    }

    pub fn testnet_markets_path(&self) -> Option<&Path> {
        self.testnet_markets_path.as_deref()
    }
}
