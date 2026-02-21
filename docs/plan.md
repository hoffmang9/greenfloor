# GreenFloor V1 Plan

## Scope

- Run a long-lived daemon (`greenfloord`) plus manager CLI (`greenfloor-manager`) for deterministic CAT/XCH market-making operations.
- Keep policy logic deterministic in `greenfloor/core`; keep side effects in adapters and CLI/daemon orchestration layers.
- Ship only low-inventory alert notifications for v1.

## Rollout Steps

1. Install project and validate baseline config in-repo (`config/program.yaml`, `config/markets.yaml`).
2. Bootstrap runtime home dir (`~/.greenfloor`) with `greenfloor-manager bootstrap-home` (required before first real deployment run).
3. Onboard signing key selection (`greenfloor-manager keys-onboard`) and verify key routing/registry mappings.
4. Start daemon (`greenfloord`) and monitor audit/state DB events for market, offer-lifecycle, and coin-op behavior.

## Syncing, Signing, and Offer Generation Baseline

- GreenFloor v1 uses the repo submodule `chia-wallet-sdk` for blockchain syncing, spend-bundle signing, and offer-file generation flows.
- Do not introduce alternate sync/sign/offer-generation stacks for default execution paths without an explicit architecture decision note.
- Keep the active default signing pipeline constrained to 3-4 layers end-to-end to reduce fragility and debugging overhead.

## Offer File Contract

- Offer files are text files containing a Bech32m offer string (prefix `offer1...`), not JSON.
- Per `chia-wallet-sdk`, offer text is an encoded/compressed `SpendBundle` (`encode_offer` / `decode_offer`).
- Adapter/test paths should treat offer files as opaque serialized artifacts: read file text, submit text to venue API, and persist IDs/status separately.

## Offer Lifecycle Strategy

- All market-making offers must always include an expiry.
- Stable-vs-unstable markets use shorter offer expiries than other pair types, so stale pricing is naturally rotated faster.
- Offer cancellation is intentionally rare and should not be a routine refresh mechanism.
- Cancellation applies only to stable-vs-unstable pairs, and only when there is strong price movement on the unstable side.
- In normal conditions, expiry-based replacement is preferred over explicit cancellation.

## Delivery Constraints

- Python 3.11+.
- Deterministic test suite (`pytest`) should stay under 10 minutes wall clock (prefer under 5).
- Required checks: `ruff check`, `ruff format --check`, `pyright`, `pytest`.

## Plan TODOs (Current State)

- [x] Baseline clarified: `chia-wallet-sdk` is the default stack for sync/sign/offer generation.
- [x] Active default signing pipeline consolidated to 4 layers and recorded in `docs/decisions/0002-signing-pipeline-consolidation.md`.
- [x] Dexie adapter offer write paths implemented (`post_offer`, `cancel_offer`) with deterministic fixture tests using real `offer1...` payloads.
- [x] Strategy port completed: legacy carbon XCH sizing logic moved into pure `greenfloor/core/strategy.py` with deterministic tests.
- [x] Coincodex price service implemented with TTL cache and stale fallback, and daemon now records XCH price snapshots each cycle.
- [x] XCH strategy is price-gated: no XCH offer planning when price snapshot is unavailable/invalid.
- [x] Home-dir bootstrap implemented via `greenfloor-manager bootstrap-home` (creates runtime layout, seeds config, initializes state DB).
- [x] P1: Wire `strategy_actions_planned` outputs into daemon offer execution path (build command contract + Dexie post + offer-state persistence).
- [x] P1-followup: Replace placeholder offer-builder command with concrete in-process `chia-wallet-sdk` offer construction via `greenfloor/cli/offer_builder_sdk.py`.
- [x] P2: Implement policy-gated cancel execution for unstable-leg markets on strong price moves using prior-vs-current XCH snapshots and threshold gating (`GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS`).
- [x] P3: Add runbook-level operator docs for first deployment and recovery workflows using home bootstrap, onboarding, and reload/history tooling (`docs/runbook.md`).

## V1.1 Backlog (Draft)

- [x] B0: Simplified bring-up path: manager supports direct build+post flow for market offers (`greenfloor-manager build-and-post-offer ...`), defaulting to mainnet unless `--network` is set.
- [x] B1: Add reconciliation pass that links posted offer IDs to venue/on-chain outcomes and flags orphaned/unknown states (`greenfloor-manager offers-reconcile`).
- [x] B2: Extend strategy policy with configurable spread/price bands sourced from market config (pricing keys: `strategy_target_spread_bps`, `strategy_min_xch_price_usd`, `strategy_max_xch_price_usd`).
- [x] B3: Add bounded retry/backoff contracts for offer post/cancel paths with explicit reason codes and cooldown windows (daemon offer post/cancel execution paths, env-tunable retry/backoff/cooldown controls, deterministic tests).
- [x] B4: Add manager command(s) to inspect recent strategy/offer execution events in a compact operator view (`greenfloor-manager offers-status`).
- [x] B5: Introduce metrics export (counts/latency/error rates) for daemon loop, offer execution, and cancel policy actions (`greenfloor-manager metrics-export` + `daemon_cycle_summary` audit events).
- [x] B6: Add deterministic integration test harness for a multi-cycle daemon scenario (price shift -> plan -> post -> cancel gate -> reconcile) in `tests/test_daemon_multi_cycle_integration.py`.
- [x] B8: Add configuration schema validation for new runtime controls/env overrides with operator-facing docs (market pricing strategy-band schema checks + `doctor` warnings for invalid runtime env overrides).

## Remaining Gaps Before First Production-Like User Test

- [ ] G1: Replace deterministic/synthetic manager offer build output with coin-backed `chia-wallet-sdk` offer construction that passes venue validation on `testnet11`.
- [ ] G2: Add operator helper workflow for `testnet11` asset discovery + inventory bootstrap (Dexie testnet liquidity discovery + market snippet generation).
- [ ] G3: Run and document an end-to-end `testnet11` proof (build -> post -> status -> reconcile) using a live test asset pair.
