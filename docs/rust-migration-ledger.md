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

- Rust `config/program.rs` and `config/markets.rs` are the only validation path
  for operator commands. Python `config/models.py` is not consulted.
- Remove `dev.python.min_version` from `program.yaml` if present; it is ignored.
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
| 2026-06-17 | Daemon tests use subprocess `greenfloord` / `greenfloor-engine daemon`           | Drop PyO3 daemon test harness imports                                            |
| 2026-06-17 | `greenfloor-engine` no longer exposes `build-and-post-offer` or `offers-*`       | Use `greenfloor-manager` for operator lifecycle commands                         |
| 2026-06-17 | `doctor` validates `signer_key_id` on enabled markets (not Python keys registry) | Ensure each enabled market has `signer_key_id` set                               |
| 2026-06-17 | `coin-split` / `coin-combine` use Rust gate policy (`coin_ops/gate.rs`)          | Until-ready requires `--size-base-units`; combine prereq auto-runs combine first |

Add a row here for every intentional operator-facing break during the migration.
