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

### 2026-06-18 — Python test harness retired; combine-market-cat-dust in Rust

Removed GreenFloor pytest suite; operator and script contract tests live in
`cargo test --manifest-path greenfloor-engine/Cargo.toml`. Added
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
