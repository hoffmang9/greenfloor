//! Probe Coinset endpoint height-window capabilities for vault scans.

mod capability;
mod cli;
mod report;
mod types;

pub use cli::{run_coinset_probe_command, CoinsetProbeCliArgs};
pub use report::build_coinset_probe_report;
pub use types::{CapabilitiesReport, HeightWindowCapability, ProbeReport, ScanWindow};
