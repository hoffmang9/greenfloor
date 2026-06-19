# Rust CLI / daemon migration ledger

Single-operator deployment catch-up checklist. No backwards-compatibility shims.

## Install

1. Build and install native binaries:

   ```bash
   cargo install --path greenfloor-engine --bins
   ```

   Installs `greenfloor-engine`, `greenfloor-manager`, and `greenfloord`.

2. Stop using `pip install -e .` for operator commands. Python venv remains for
   `scripts/` and dev tooling only.

## Command invocation

| Before                                      | After                                            |
| ------------------------------------------- | ------------------------------------------------ |
| `pip` console script `greenfloor-manager …` | `greenfloor-manager …` (native binary)           |
| `pip` console script `greenfloord …`        | `greenfloord …` (native binary)                  |
| `build-and-post-offer` via PyO3 in-process  | `greenfloor-manager build-and-post-offer` (Rust) |

Global flags on `greenfloor-manager`:

- `--program-config` (default `~/.greenfloor/config/program.yaml` when present)
- `--markets-config`, `--testnet-markets-config`, `--cats-config`, `--state-db`
- `--json` for compact single-line JSON on supported commands

## Config / state

- Rust `config/program.rs`, `config/markets.rs`, and `config/signer.rs` are the only
  operator config policy path. `greenfloor-manager config-validate` is the operator gate.
- Scripts call native `greenfloor-manager` commands for validated fields (`program-fields`,
  `markets-fields`, `cats-fields`), test program materialization (`materialize-minimal-program`),
  and `config-validate`. Scripts must not reimplement YAML policy walks for those fields.
- `dev.python.min_version` is optional in `program.yaml`; when omitted, Rust defaults to `3.11`.
- State DB schema is owned by `greenfloor-engine/src/storage/`; run `doctor` after
  upgrade to confirm SQLite opens.

## Deployment catch-up (after pulling migration)

1. `cargo install --path greenfloor-engine --bins`
2. Review `program.yaml` / `markets.yaml` against repo templates; fix any fields
   `greenfloor-manager config-validate` rejects.
3. `greenfloor-manager config-validate`
4. `greenfloor-manager doctor` (exit 0 required)
5. Restart `greenfloord`
6. `greenfloord --once` smoke cycle

## Breaking changes log

| Date       | Change                                                                           | Action                                                                           |
| ---------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| 2026-06-17 | Python `greenfloor-manager` / `greenfloord` entrypoints removed                  | Install Cargo binaries                                                           |
| 2026-06-17 | Manager `build-and-post-offer` is Rust-only (no PyO3)                            | Use `greenfloor-manager build-and-post-offer`                                    |
| 2026-06-17 | `greenfloor-engine-pyo3` deleted; Coinset IO via `greenfloor-engine coinset …`   | `cargo install --path greenfloor-engine --bins` for scripts                      |
| 2026-06-17 | Daemon tests use `greenfloor-engine daemon-once --request-json`                  | Set `GREENFLOOR_DAEMON_TEST_CONTROLS=1` when using non-default `test_controls`   |
| 2026-06-17 | `greenfloor-engine` no longer exposes `build-and-post-offer` or `offers-*`       | Use `greenfloor-manager` for operator lifecycle commands                         |
| 2026-06-17 | `doctor` validates `signer_key_id` on enabled markets (not Python keys registry) | Ensure each enabled market has `signer_key_id` set                               |
| 2026-06-17 | `coin-split` / `coin-combine` use Rust gate policy (`coin_ops/gate.rs`)          | Until-ready requires `--size-base-units`; combine prereq auto-runs combine first |
| 2026-06-17 | Python `config/models.py` deleted; script config via manager field CLIs          | Call `greenfloor-manager` field commands; do not walk operator YAML for policy   |
| 2026-06-17 | `markets-fields` exports `markets` (all) and `enabled_markets`                   | Scripts needing disabled-market metadata use `markets`; operators use `enabled`  |

Add a row here for every intentional operator-facing break during the migration.
