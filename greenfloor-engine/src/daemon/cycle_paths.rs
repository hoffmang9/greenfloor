use std::path::{Path, PathBuf};

use crate::offer::operator::OperatorConfigPaths;

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

    /// Paths shape shared with manager/ensure `build_and_post` callers.
    #[must_use]
    pub fn as_operator_paths(&self) -> OperatorConfigPaths {
        OperatorConfigPaths {
            program_path: self.program_path.clone(),
            markets_path: self.markets_path.clone(),
            testnet_markets_path: self.testnet_markets_path.clone(),
        }
    }
}
