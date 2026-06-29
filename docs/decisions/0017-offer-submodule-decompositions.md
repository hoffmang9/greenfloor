# ADR 0017: Offer bootstrap and presplit submodule decompositions

## Status

Accepted (2026-06-29)

## Context

Three refactors decomposed large `offer/` modules without changing operator JSON or
on-chain behavior:

1. **Bootstrap planner (#155)** — `planner.rs` mixed domain types, orchestration, and tests.
2. **Presplit (#156)** — `presplit/mod.rs` mixed binding, build, conditions, split, pipeline,
   and cancel-binding peel/parse logic.
3. **Bootstrap phase (#157)** — `phase.rs` mixed typed phase snapshots, wait polling, and
   offer-creation gating imports.

## Decision

### Bootstrap layout (`offer/bootstrap/`)

| Module                                         | Responsibility                                                                                                                |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `plan.rs`                                      | Domain model: `BootstrapPlan`, `BootstrapPlanOutcome`, ladder rows, coins; `BootstrapPlanOutcome::combine_first_pending()`    |
| `planner.rs`                                   | Orchestration only: `plan_bootstrap_mixed_outputs`                                                                            |
| `planner/tests.rs`                             | Planner unit tests                                                                                                            |
| `test_fixtures.rs`                             | Shared planner/phase/replan test helpers (`plan_bootstrap`, `expect_needs_shape`, …)                                          |
| `phase/mod.rs`                                 | `BootstrapPhaseStatus`, `BootstrapPhaseSnapshot`, early/executed phase mapping, shape wait poll types/resolution              |
| `phase/tests.rs`                               | Phase policy unit tests                                                                                                       |
| `gate.rs`                                      | Offer-creation gating policy: `BootstrapOfferGate`, `bootstrap_offer_gate_for_status`, `bootstrap_phase_snapshot_block_error` |
| `shape_policy.rs`, `replan.rs`, `ladder.rs`, … | Unchanged roles; use `combine_first_pending()` where outcome-level combine-first checks apply                                 |

**Ownership split (extends ADR 0014):**

- **`phase/`** constructs typed snapshots (`BootstrapPhaseStatus` on `BootstrapPhaseSnapshot`).
- **`gate/`** interprets snapshots/results for offer build/post blocking (`BootstrapPhaseStatus`
  → `BootstrapOfferGate` → block error string).
- Dependency direction is **`gate` → `phase`** (no cycle).

**Snapshot block errors:** call `bootstrap_phase_snapshot_block_error(&snapshot)` (re-exported from
`offer::bootstrap`). Operator results continue to use
`BootstrapPhaseResult::offer_creation_block_error()`.

### Presplit layout (`offer/presplit/`)

| Module                    | Responsibility                                                                  |
| ------------------------- | ------------------------------------------------------------------------------- |
| `binding.rs`              | Offer binding verification                                                      |
| `build.rs`                | Offer build from presplit CAT                                                   |
| `conditions.rs`           | Inner spend conditions, including `build_presplit_offer_cancel_inner_spend`     |
| `split.rs`                | Split spend bundle construction                                                 |
| `pipeline.rs`             | `PresplitPaymentContext` — shared payment inputs across plan/input-spend/encode |
| `cancel_binding/mod.rs`   | Cancel binding lookup and verification                                          |
| `cancel_binding/parse.rs` | Parse cancel binding from coin input                                            |
| `cancel_binding/peel.rs`  | Puzzle peel helpers                                                             |
| `mod.rs`                  | Barrel re-exports only                                                          |

## Consequences

- File-path references in older docs (`planner.rs` monolith, `phase.rs`, `presplit.rs`) are
  historical; use this ADR for navigation.
- Library consumers must not call removed `BootstrapPhaseSnapshot::offer_creation_block_error()`;
  use `bootstrap_phase_snapshot_block_error()` instead (see `rust-migration-ledger.md`).
- Operator CLI JSON and cancel/reclaim behavior are unchanged.

## References

- [0014](0014-offer-publish-module-decomposition.md) — publish decomposition and original bootstrap gate collapse
- [0015](0015-on-chain-offer-cancel.md) — presplit cancel inner spend (now in `presplit/conditions.rs`)
