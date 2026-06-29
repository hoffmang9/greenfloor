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

### 2026-06-29 — Coinset parse and pagination decomposed (#158)

Split monolithic `coinset/parse.rs` into `parse/{payload,record,tests}.rs`; extracted
`rpc_result`, `json_util`, and `batch`. Split `pagination.rs` into `pagination/` with cursor
parsing in `cursor.rs` and async page orchestration in `mod.rs`. Unified typed and JSON RPC
success checks; inlined unspent `CoinRecord` filtering; adopted `to_coinset_hex` across in-crate
callers. Public `greenfloor_engine::coinset::*` re-exports unchanged. See ADR 0018.

### 2026-06-29 — Bootstrap phase mapping refactored (#157)

Decomposed `offer/bootstrap/phase.rs` into `phase/mod.rs` + `phase/tests.rs`. Typed
`BootstrapPhaseStatus` on snapshots; shared `outcome_reason` mapping; `combine_first_pending()`
on `BootstrapPlanOutcome`; unified after-combine/split wait completion helpers. Snapshot
block-error gating moved to `bootstrap_phase_snapshot_block_error()` in `gate.rs` (one-way
`gate` → `phase`). See ADR 0017.

### 2026-06-29 — Presplit module decomposed (#156)

Split `offer/presplit/mod.rs` into `binding`, `build`, `conditions`, `split`, `pipeline`, and
`cancel_binding/{mod,parse,peel}.rs`. `PresplitPaymentContext` deduplicates payment inputs
across plan/input-spend/encode. Cancel inner spend remains
`build_presplit_offer_cancel_inner_spend` in `conditions.rs`. See ADR 0017.

### 2026-06-29 — Bootstrap planner decomposed (#155)

Extracted domain model to `bootstrap/plan.rs`; slim orchestration in `planner.rs`;
`planner/tests.rs` and shared `test_fixtures.rs` for planner/phase/replan tests. Centralized
ladder deficit collection and `BootstrapPlan::needs_shape` invariants. See ADR 0017.

### 2026-06-22 — On-chain offer cancel hardened (ADR 0015)

Shared cancel/reclaim spend builder (`offer/reclaim.rs`); Dexie is offer-file fetch only;
Coinset broadcast. Presplit cancel binding extracted from offer input spend (no replan).
Operator state uses `cancel_submitted` until reconcile confirms `cancelled`. CLI JSON field
`submitted_count` replaces `cancelled_count`. Simulator e2e test for presplit-existing
`build_offer_cancel_spend_bundle`.

### 2026-06-21 — Offer publish module decomposed; bootstrap gate collapsed

Split `offer/publish/mod.rs` into venue-focused `publish/dexie/` and `publish/assets/`
(expectations + Dexie visibility). Bootstrap offer-creation gating policy moved to
`offer/bootstrap/gate.rs`; typed phase snapshots later refined in ADR 0017 (#157). Operator
block checks use `BootstrapPhaseResult::offer_creation_block_error()`; removed wrapper
re-exports from `offer::`. See ADR 0014 and `rust-migration-ledger.md` for library-only
breaking changes.

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
- Architecture decisions: [`README.md`](README.md) (start with ADR 0013, ADR 0015 for cancel, ADR 0017 for offer module layout)
- Breaking changes / migration catch-up: [`rust-migration-ledger.md`](rust-migration-ledger.md)
