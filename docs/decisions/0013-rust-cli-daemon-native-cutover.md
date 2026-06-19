# ADR 0013: Rust-native CLI and daemon cutover

## Status

Accepted (2026-06-17)

## Context

GreenFloor has one operator deployment. At cutover time the daemon already ran through
`greenfloor-engine daemon`, but operator entrypoints, several manager commands, config
validation, and large Python CLI/daemon packages remained.

An intermediate cutover plan assumed a long-lived Python manager wrapper and Python daemon
orchestration. That path is obsolete: the Rust engine owns daemon cycles, offer build/post,
policy, SQLite, and most adapters. Superseded intermediate ADRs (`0005`–`0012`) were removed
from the index; see git history for the full record.

## Decision

1. **Production operator runtime is Rust-only.** `greenfloor-manager` and
   `greenfloord` are native binaries (Cargo `[[bin]]` targets) that delegate to
   `greenfloor-engine` command implementations. No Python console scripts for
   daemon or manager paths.

2. **No backwards compatibility.** Command flags, JSON output shapes, and YAML
   config fields may change when the Rust shape is simpler. Every intentional
   break is recorded in [docs/rust-migration-ledger.md](../rust-migration-ledger.md)
   with deployment catch-up steps.

3. **No PyO3 on any path.** Operators and scripts use native Rust binaries only.

   **Coinset mutation IO for scripts:** nested `greenfloor-engine coinset` subcommands:
   - `coinset post` (reads and fee RPCs)
   - `coinset push-tx`

   **Integration tests:** `greenfloor-engine daemon-once --request-json <file> --json`
   (includes `test_controls` in the request body; no hidden flags on `greenfloord`).

   **Removed (2026-06-17):** `greenfloor-engine-pyo3/`, Python policy bridges, and all
   in-process Python↔Rust FFI.

   **Import convention:** operator binaries import `manager_cli`, `daemon::cli`, and
   `coinset_cli` directly.

4. **Python scripts stay.** Standalone utilities under `scripts/` use script-only Python
   libraries (`scripts/greenfloor_scripts/` subprocess adapters) and must not reimplement operator YAML
   policy walks. Config field reads go through `greenfloor-manager program-fields`,
   `markets-fields`, and `cats-fields`.

5. **Quality bar.** Implementation work is held to the `thermonuclear-code-review`
   skill standard. A manager agent splits work into subagent-sized packets and
   loops implement → test → review until only two or fewer nit findings remain.

6. **Policy parity safety net.** With PyO3 and Python policy bridges removed,
   operator correctness is enforced by
   `cargo nextest run --manifest-path greenfloor-engine/Cargo.toml` in CI. Script
   subprocess adapters are covered by `unittest` invoked from
   `greenfloor-engine/tests/script_adapter_subprocess.rs`; they do not re-test Rust
   policy or conservative-fee parsing.

## Command ownership

| Operator command                                               | Owner                               |
| -------------------------------------------------------------- | ----------------------------------- |
| `greenfloord` / `daemon`                                       | `greenfloor-engine`                 |
| `config-validate`, `doctor`, `bootstrap-home`, `set-log-level` | `greenfloor-manager` (Rust)         |
| `program-fields`, `markets-fields`, `cats-fields`              | `greenfloor-manager` (Rust JSON)    |
| `materialize-minimal-program`                                  | `greenfloor-manager` (test/fixture) |
| `build-and-post-offer`, `offers-*`                             | `greenfloor-manager` → Rust engine  |
| `coins-list`, `coin-status`, `coin-split`, `coin-combine`      | `greenfloor-manager` (Rust)         |
| `combine-market-cat-dust`                                      | `greenfloor-manager` (Rust)         |
| `keys-onboard`, `cats-*`                                       | `greenfloor-manager` (Rust)         |

## Consequences

- Operators install via `cargo install --path greenfloor-engine` (or CI-built
  artifacts) instead of `pip install` console scripts.
- Python `greenfloor/cli/`, `greenfloor/daemon/`, and CLI-only runtime modules
  were deleted at cutover (2026-06-17).
- Docs, runbook, and CI reference native binaries only.
- Deployment updates are driven by the migration ledger, not compatibility shims.
