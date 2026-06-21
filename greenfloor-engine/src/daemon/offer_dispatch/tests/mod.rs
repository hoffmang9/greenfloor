//! Offer-dispatch tests split by concern:
//! - `classify_tests` / `coordinator_tests`: pure policy and coordinator logic
//! - `test_overrides`: injection mapper unit tests (branch table only)
//! - `harness_tests`: wiring through `execute_strategy_actions` (assert exit counts only)

mod classify_tests;
mod coordinator_tests;
mod fixtures;
mod harness;
mod harness_tests;
