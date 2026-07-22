//! In-process daemon loop lifecycle harness.
//!
//! Canonical pattern: see [`crate::test_support::injections`].

use std::time::Duration;

use crate::daemon::run_once::DaemonCycleTestControls;

/// Test-only controls for [`super::daemon_loop::run_daemon_loop_with_harness`].
///
/// The harness path never starts the background Coinset websocket thread.
#[derive(Debug, Clone)]
pub struct DaemonLoopTestHarness {
    pub max_cycles: usize,
    pub cycle_sleep: Duration,
    pub cycle_test_controls: DaemonCycleTestControls,
}

impl Default for DaemonLoopTestHarness {
    fn default() -> Self {
        Self {
            max_cycles: 1,
            cycle_sleep: Duration::ZERO,
            cycle_test_controls: DaemonCycleTestControls {
                skip_strategy_execution: true,
                ..DaemonCycleTestControls::default()
            },
        }
    }
}

impl DaemonLoopTestHarness {
    #[must_use]
    pub fn with_cycles(max_cycles: usize) -> Self {
        Self {
            max_cycles,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::DaemonLoopTestHarness;
    use crate::daemon::daemon_loop::{run_daemon_loop_with_harness, DaemonLoopRequest};
    use crate::daemon::reload::reload_marker_path;
    use crate::daemon::run_once::DaemonCycleTestControls;
    use crate::minimal_program_template::{write_minimal_program, MinimalProgramParams};
    use crate::operator_log::{CONFIG_RELOADED, DAEMON_CYCLE_SUMMARY};
    use crate::storage::SqliteStore;

    struct LoopFixture {
        _dir: tempfile::TempDir,
        request: DaemonLoopRequest,
        db_path: PathBuf,
        state_dir: PathBuf,
    }

    impl LoopFixture {
        fn new(dexie_base: &str) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let home = dir.path().join("home");
            let state_dir = home.join("state");
            std::fs::create_dir_all(&state_dir).expect("state dir");
            let program = dir.path().join("program.yaml");
            let markets = dir.path().join("markets.yaml");
            let db_path = dir.path().join("state.sqlite");
            write_minimal_program(
                &program,
                MinimalProgramParams {
                    home_dir: &home,
                    dexie_api_base: dexie_base,
                    ..Default::default()
                },
            );
            write_single_market_yaml(&markets);
            let request = DaemonLoopRequest {
                program_path: program,
                markets_path: markets,
                testnet_markets_path: None,
                state_db_override: Some(db_path.display().to_string()),
                // Unused: harness path does not start the Coinset websocket loop.
                coinset_base_url: "http://127.0.0.1:9".to_string(),
                state_dir: state_dir.clone(),
                allowed_key_ids: Vec::new(),
            };
            Self {
                _dir: dir,
                request,
                db_path,
                state_dir,
            }
        }

        async fn run(&self, harness: DaemonLoopTestHarness) -> i32 {
            Box::pin(run_daemon_loop_with_harness(self.request.clone(), harness))
                .await
                .expect("daemon loop harness")
        }
    }

    fn write_single_market_yaml(path: &Path) {
        let yaml = r#"markets:
  - id: m1
    enabled: true
    base_asset: "asset1"
    base_symbol: "AS1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 10
      bucket_counts:
        1: 0
    ladders:
      sell:
        - size_base_units: 1
          target_count: 1
          split_buffer_count: 0
          combine_when_excess_factor: 2.0
"#;
        std::fs::write(path, yaml).expect("write markets yaml");
    }

    fn audit_event_count(store: &SqliteStore, event_type: &str) -> usize {
        store
            .list_recent_audit_events(Some(&[event_type]), None, 20)
            .expect("audit events")
            .len()
    }

    async fn fixture_with_dexie_offers() -> (mockito::ServerGuard, LoopFixture) {
        let mut server = mockito::Server::new_async().await;
        let _offers = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"/v1/offers\?.*".to_string()),
            )
            .with_status(200)
            .with_body(r#"{"success":true,"offers":[]}"#)
            .create_async()
            .await;
        let fixture = LoopFixture::new(&server.url());
        (server, fixture)
    }

    #[tokio::test]
    async fn loop_runs_configured_cycle_count_and_returns_last_exit_code() {
        let (_server, fixture) = fixture_with_dexie_offers().await;
        let exit_code = fixture.run(DaemonLoopTestHarness::with_cycles(2)).await;
        assert_eq!(exit_code, 0);
        let store = SqliteStore::open(&fixture.db_path).expect("open db");
        assert_eq!(audit_event_count(&store, DAEMON_CYCLE_SUMMARY), 2);
    }

    #[tokio::test]
    async fn loop_returns_non_zero_when_last_cycle_fails() {
        let (_server, fixture) = fixture_with_dexie_offers().await;
        let exit_code = fixture
            .run(DaemonLoopTestHarness {
                max_cycles: 1,
                cycle_test_controls: DaemonCycleTestControls {
                    skip_strategy_execution: true,
                    force_market_error_for: Some("m1".to_string()),
                    ..DaemonCycleTestControls::default()
                },
                ..DaemonLoopTestHarness::default()
            })
            .await;
        assert_eq!(exit_code, 1);
    }

    #[tokio::test]
    async fn loop_clears_reload_marker_during_cycle() {
        let (_server, fixture) = fixture_with_dexie_offers().await;
        std::fs::write(
            reload_marker_path(&fixture.state_dir),
            br#"{"reload_id":"reload-loop-1"}"#,
        )
        .expect("write reload marker");
        fixture.run(DaemonLoopTestHarness::default()).await;
        assert!(!reload_marker_path(&fixture.state_dir).is_file());
        let store = SqliteStore::open(&fixture.db_path).expect("open db");
        assert_eq!(audit_event_count(&store, CONFIG_RELOADED), 1);
    }
}
