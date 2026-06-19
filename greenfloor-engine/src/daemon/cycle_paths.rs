use std::path::{Path, PathBuf};

/// Config file paths carried through a daemon cycle (from the run-once request).
#[derive(Debug, Clone)]
pub struct DaemonCyclePaths {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
}

impl DaemonCyclePaths {
    #[must_use]
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

    #[must_use]
    pub fn testnet_markets_path(&self) -> Option<&Path> {
        self.testnet_markets_path.as_deref()
    }
}
