//! Test-only utilities exposed for integration tests and in-crate unit tests.

#[doc(hidden)]
pub mod env_restore_guard;

#[doc(hidden)]
pub use env_restore_guard::EnvRestoreGuard;
