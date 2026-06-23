//! Setup, validation, and health commands for the manager CLI.

mod audit_prune;
mod bootstrap;
mod doctor;
mod fields;
mod log_level;
mod materialize;
mod validate;

pub use audit_prune::run_audit_prune;

pub use bootstrap::{run_bootstrap_home, BootstrapHomeParams};
pub use doctor::run_doctor;
pub use fields::{run_cats_fields, run_markets_fields, run_program_fields};
pub use log_level::run_set_log_level;
pub use materialize::{
    run_materialize_minimal_program, MaterializeMinimalProgramFeatureFlags,
    MaterializeMinimalProgramRequest,
};
pub use validate::run_config_validate;
