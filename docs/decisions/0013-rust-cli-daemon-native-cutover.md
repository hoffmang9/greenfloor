# ADR 0013: Rust-native CLI and daemon cutover

## Status

Accepted (2026-06-17)

## Context

GreenFloor has one operator deployment. The daemon already runs through
`greenfloor-engine daemon`, but operator entrypoints, several manager commands,
config validation, and large Python CLI/daemon packages remain.

ADR 0012 assumed a long-lived Python manager wrapper and Python daemon
orchestration. That is obsolete: the Rust engine owns daemon cycles, offer
build/post, policy, SQLite, and most adapters.

## Decision

1. **Production operator runtime is Rust-only.** `greenfloor-manager` and
   `greenfloord` are native binaries (Cargo `[[bin]]` targets) that delegate to
   `greenfloor-engine` command implementations. No Python console scripts for
   daemon or manager paths.

2. **No backwards compatibility.** Command flags, JSON output shapes, and YAML
   config fields may change when the Rust shape is simpler. Every intentional
   break is recorded in [docs/rust-migration-ledger.md](../rust-migration-ledger.md)
   with deployment catch-up steps.

3. **PyO3 is dev/test-only.** `greenfloor-engine-pyo3` remains for CI parity and
   integration tests; operators install and run Rust binaries only.

   **Import convention:** new PyO3 bindings should prefer domain module paths
   (`offer::`, `daemon::`, `cycle::`, …) over flattened crate-root re-exports in
   `lib.rs`. Operator binaries import `manager_cli` and `daemon::cli` directly.

4. **Python scripts stay.** Standalone utilities under `scripts/` may keep using
   script-only Python libraries (`config`, `adapters`, `hex_utils`) until
   explicitly ported.

5. **Quality bar.** Implementation work is held to the `thermonuclear-code-review`
   skill standard. A manager agent splits work into subagent-sized packets and
   loops implement → test → review until only two or fewer nit findings remain.

## Command ownership

| Operator command                                               | Owner                              |
| -------------------------------------------------------------- | ---------------------------------- |
| `greenfloord` / `daemon`                                       | `greenfloor-engine`                |
| `config-validate`, `doctor`, `bootstrap-home`, `set-log-level` | `greenfloor-manager` (Rust)        |
| `build-and-post-offer`, `offers-*`                             | `greenfloor-manager` → Rust engine |
| `coins-list`, `coin-status`, `coin-split`, `coin-combine`      | `greenfloor-manager` (Rust)        |
| `keys-onboard`, `cats-*`                                       | `greenfloor-manager` (Rust)        |

## Consequences

- Operators install via `cargo install --path greenfloor-engine` (or CI-built
  artifacts) instead of `pip install` console scripts.
- Python `greenfloor/cli/`, `greenfloor/daemon/`, and CLI-only runtime modules
  are deleted once Rust commands and tests land.
- Docs, runbook, and CI reference native binaries only.
- Deployment updates are driven by the migration ledger, not compatibility shims.
