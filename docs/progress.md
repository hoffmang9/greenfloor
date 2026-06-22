# Progress Log

Recent milestones and live testing targets.

**Canonical scope:** architecture, shipped baseline, open items, and delivery constraints
are in [`plan.md`](plan.md). Agent coding policy is in [`AGENTS.md`](../AGENTS.md).

Pre-Rust migration detail lives in git history and
[`rust-migration-ledger.md`](rust-migration-ledger.md).

## Active live testing

- **Mainnet canary:** `eco1812022_sell_wusdbc` (`ECO.181.2022:wUSDC.b`). See runbook
  §2 mainnet cutover checklist.
- **Testnet11 proof pair:** `TDBX:txch` (CI via `live-testnet-e2e.yml`).

## Milestones

### 2026-06-21 — Vault coinset scan checkpoint module decomposed

Split the largest non-test Rust file into `checkpoint/{runtime,file,load,save}` with typed
`LoadCheckpointResult` load outcomes. `ScanState` embeds `LoadedCheckpoint` as live resume
payload; scan orchestration metadata lives in `ScanCheckpointContext`.

**Operator JSON change:** when an on-disk checkpoint is discarded because request params
differ, `checkpoint.discard_reason` is now specific (`launcher_id_mismatch`,
`network_mismatch`, `include_spent_mismatch`) instead of generic
`checkpoint_params_mismatch`.

### 2026-06-21 — Test coverage gaps closed; test injection gated behind `cfg(test)` (#118)

In-process harness tests for parallel offer dispatch, coinset CLI dispatch, Dexie cats
lookup, coin-op split, build-offer CLI wiring, and vault session KMS resolution. Test
override fields and branches stripped from release builds via `#[cfg(test)]` on coin-op,
offer, and dispatch paths. CI/Cargo target-dir alignment shipped separately in #117.

### 2026-06-21 — Cargo target dir aligned with CI rust cache (#117)

`.cargo/config.toml` points `target-dir` at `greenfloor-engine/target`; Swatinem restores
the directory Cargo writes. Scripts/e2e resolve the path via `cargo_target_directory()` in
`binaries.py`. Lint and e2e workflows skip unnecessary `cargo-nextest` installs.

### 2026-06-19 — Project agent skills documented

Added [`coverage-review`](../.cursor/skills/coverage-review/SKILL.md) — analyse test coverage
gaps and report uncovered code before making changes (`/coverage-review`). Documented existing
[`check-commit-signature`](../.cursor/skills/check-commit-signature/SKILL.md) in
[`AGENTS.md`](../AGENTS.md) → **Agent skills**.

### 2026-06-18 — Python test harness retired; combine-market-cat-dust in Rust

Removed GreenFloor pytest suite; operator and script contract tests live in
`cargo nextest run --manifest-path greenfloor-engine/Cargo.toml` (CI;
`cargo test` with the same manifest also works locally). Added
`greenfloor-manager combine-market-cat-dust` (vault scan + dust filter + `coin-combine`
batches). CAT parse replay uses production `Cat::parse_children` path via opt-in
`GREENFLOOR_CAT_PARSE_REPLAY_CASES_DIR`.

### 2026-06-17 — Rust-native operator cutover

Single cutover (ADR 0013): native `greenfloor-manager` and `greenfloord`; PyO3 and Python
orchestration removed; Rust owns config policy, signing, offers, coin ops, daemon cycles, and
SQLite. Scripts use `greenfloor-engine coinset …` and manager field CLIs. ADR index trimmed
to active decisions (`0013`, `0010`, `0007`, `0003`).

## References

- V1 scope: [`plan.md`](plan.md)
- Operator procedures: [`runbook.md`](runbook.md)
- Architecture decisions: [`README.md`](README.md) (start with ADR 0013)
- Breaking changes / migration catch-up: [`rust-migration-ledger.md`](rust-migration-ledger.md)
