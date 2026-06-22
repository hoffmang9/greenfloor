//! Probe Coinset endpoint height-window capabilities for vault scans.

mod cli;
mod command;
mod report;
mod types;

pub use cli::CoinsetProbeCliArgs;
pub use command::run_coinset_probe_command;
pub use report::build_coinset_probe_report;
pub use types::{
    CapabilitiesReport, EndpointCapability, NamesCapability, ProbeAttempt, ProbeReport, ScanWindow,
};
