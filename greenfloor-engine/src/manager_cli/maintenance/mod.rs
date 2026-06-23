//! Maintenance commands for operator DB hygiene.

mod audit_prune;

pub use audit_prune::run_audit_prune;
