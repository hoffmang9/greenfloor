# Progress Log

Current architecture and recent milestones. Older migration history lives in git
history and superseded ADRs (`0006`–`0012`).

## Current architecture (2026-06)

**Operators (production):** native Rust binaries only.

| Binary               | Role                                                       |
| -------------------- | ---------------------------------------------------------- |
| `greenfloor-manager` | Config, keys, cats, coin ops, build/post, offers lifecycle |
| `greenfloord`        | Market cycle daemon (`--once` or loop)                     |
| `greenfloor-engine`  | Low-level engine CLI (vault debug, legacy subcommands)     |

Implementation lives in `greenfloor-engine/src/`:

- `config/` — program, markets, and signer parse/validation (operator policy)
- `manager_cli/` — manager command dispatch and JSON output
- `daemon/` — cycle loop, market phases, websocket tx signals
- `offer/operator/` — shared build/post + signer denomination (manager + daemon)
- `offer/lifecycle/` — reconcile, cancel, status (manager + daemon)
- `coin_ops/` + `daemon/coin_ops_execution/` — coin-op policy and execution
- `cycle/` — deterministic strategy, cancel policy, parallel dispatch
- `storage/` — SQLite schema and persistence

- Python (`greenfloor/` + `scripts/`): config CLI adapters (`greenfloor/config/io.py` →
  `greenfloor-manager program-fields`, `markets-fields`, `cats-fields`,
  `materialize-minimal-program`, `config-validate`), hex helpers, and Coinset adapter
  for standalone scripts. Operator config policy and validation are Rust-only
  (`greenfloor-engine/src/config/`).

**Deleted:** `greenfloor-engine-pyo3/`, `greenfloor/core/`, policy bridges, PyO3 FFI.

**Deleted:** `greenfloor/cli/`, `greenfloor/daemon/`, Python offer/coin-op orchestration
runtime modules.

## Recent milestones

### 2026-06-17 — Rust config policy; script CLI field adapters

- Deleted `greenfloor/config/models.py` and Python config policy pytest mirrors; Rust owns
  program/markets/signer parse and validation (`greenfloor-engine/src/config/`).
- Added script-facing manager commands: `program-fields`, `markets-fields`, `cats-fields`,
  `materialize-minimal-program` (JSON with `--json` where applicable).
- Python `greenfloor/config/io.py` shells out to those commands; scripts must not walk
  operator YAML for policy fields (`launcher.py`, `combine_market_cat_dust_coinset.py`,
  `vault_coinset_scan_lib.py`).
- Unified program YAML load: `read_program_yaml` → `parse_program_config` /
  `parse_signer_config`; execution paths use `load_program_bundle_gated` and
  `signer_for_execution()` with stable skip reasons.
- Test safety net: `greenfloor-engine/tests/config/`, `tests/manager_integration/`; shared
  minimal program template in `minimal_program_template.rs`.
- Pytest: script adapters and subprocess harnesses (~52 tests); policy parity is
  `cargo test` in `greenfloor-engine/`.

### 2026-06-17 — Rust-centric CI/pre-commit; Python scope trimmed

- Added `cargo fmt --check` and `cargo clippy` to pre-commit and CI (ubuntu + arm).
- `cargo test` runs on `ubuntu-24.04-arm` as well as `ubuntu-latest`.
- Removed Python `SqliteStore` test helpers; daemon assertions use `daemon-once` JSON responses.
- `live-testnet-e2e` no longer installs `chia-wallet-sdk` PyO3 wheel.

### 2026-06-17 — PyO3 removed; Coinset CLI for scripts

- Deleted `greenfloor-engine-pyo3/`; scripts use nested `greenfloor-engine coinset …` subcommands.
- `greenfloor/adapters/coinset.py` shells out to the native binary for push/fee IO.
- Moved `storage/sqlite.py` to `tests/helpers/sqlite_store.py` (test-only).
- Daemon integration tests use `greenfloor-engine daemon-once --request-json` with
  `GREENFLOOR_DAEMON_TEST_CONTROLS=1` for non-default `test_controls`.

### 2026-06-17 — Rust-native CLI/daemon cutover (ADR 0013)

- Native `greenfloor-manager` and `greenfloord`; Python console scripts removed.
- All V1 manager commands in `manager_cli/`; daemon cycle fully in Rust.
- Migration catch-up: `docs/rust-migration-ledger.md`.

### 2026-06-17 — Module boundary cleanup

- Removed `manager/` shim; shared orchestration in `offer/operator` and `offer/lifecycle`.
- Signer denomination decomposed under `offer/operator/signer_denomination/`.
- Unified manager JSON output (`emit_json` / `emit_serialized`); coin-op errors return
  payloads to command boundary instead of emitting from mid-stack helpers.
- Trimmed crate-root re-exports in `lib.rs`; operator binaries import domain modules
  directly.

## Active live testing

- **Mainnet canary:** `eco1812022_sell_wusdbc` (`ECO.181.2022:wUSDC.b`). See runbook
  §2 mainnet cutover checklist.
- **Testnet11 proof pair:** `TDBX:txch` (historical G1–G3 closure; CI via
  `live-testnet-e2e.yml`).

## References

- Operator procedures: `docs/runbook.md`
- V1 scope and open items: `docs/plan.md`
- Breaking changes: `docs/rust-migration-ledger.md`
- Architecture decisions: `docs/decisions/` (start with ADR 0013)
