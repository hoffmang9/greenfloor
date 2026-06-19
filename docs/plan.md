# GreenFloor V1 Plan

## Scope

- Long-lived daemon (`greenfloord`) plus manager CLI (`greenfloor-manager`) for
  deterministic CAT/XCH market-making.
- Policy and execution in Rust (`greenfloor-engine`); slim Python package for config CLI
  adapters (`greenfloor/config/io.py`), hex/Coinset helpers, and `scripts/` utilities.
- V1 notifications: low-inventory alerts only (ticker, remaining amount, receive address).

## Architecture

```
Operators                greenfloor-engine (Rust)
─────────                ─────────────────────────
greenfloor-manager  ──►  manager_cli/ → offer/operator, offer/lifecycle, coin_ops/…
greenfloord         ──►  daemon/      → cycle/, offer/operator, coin_ops/execution/…

Dev / tests              greenfloor (Python)
─────────                ─────────────────
parity tests, scripts ──► greenfloor-manager field CLIs + `greenfloor-engine` (`coinset …`, `daemon-once`)
```

- **Canonical signing and offer build:** `greenfloor-engine` (vault KMS + Coinset MSP).
- **Config policy for operators:** Rust (`config/program.rs`, `config/markets.rs`, `config/signer.rs`).
- **Script-facing config reads:** `greenfloor-manager program-fields`, `markets-fields`,
  `cats-fields` (via `greenfloor/config/io.py`); not direct YAML policy walks.
- **State DB:** Rust (`storage/`); SQLite at `~/.greenfloor/db/greenfloor.sqlite`.
- **No PyO3** in the repository (ADR 0013).

Legacy `cloud_wallet:` blocks in `program.yaml` are rejected; use `signer:` + `vault:`.

## Operator commands

Core trading/runtime (V1):

1. `bootstrap-home` — create `~/.greenfloor` layout and seed configs
2. `config-validate` — validate program + markets YAML
3. `doctor` — readiness check (config, keys, DB, env overrides)
4. `keys-onboard` — key selection and onboarding state
5. `build-and-post-offer` — vault KMS offer build + Dexie/Splash publish
6. `offers-status` — offer states and recent audit events
7. `offers-reconcile` — refresh states from venue + Coinset tx signals
8. `offers-cancel` — cancel by offer id or `--cancel-open`
9. `coins-list` / `coin-status` — vault coin inventory via Coinset
10. `coin-split` / `coin-combine` — denomination shaping (default waits for confirmation)

Adjunct operator commands:

- `cats-add`, `cats-list`, `cats-delete` — CAT catalog in `cats.yaml`
- `set-log-level` — update `app.log_level` in program config

Script and test adapter commands (JSON with `--json` unless noted):

- `program-fields` — script-facing program/signer/vault summary fields
- `markets-fields` — all `markets` rows plus `enabled_markets`
- `cats-fields` — CAT catalog rows and `symbol_to_asset_id` map
- `materialize-minimal-program` — write shared test minimal `program.yaml` template

Global flags: `--program-config`, `--markets-config`, `--testnet-markets-config`,
`--cats-config`, `--state-db`, `--json` (compact JSON), `--dexie-base-url`.

Coin-op notes:

- Default output is pretty JSON; `--json` emits compact single-line JSON.
- `--until-ready` requires `--size-base-units`; bounded by `--max-iterations`.
- `--no-wait` submits without waiting for confirmation.
- Fee preflight runs before coin-op submission (see runbook incident triage).

## Offer policy

- All posted offers include expiry; stable-vs-unstable pairs use shorter expiries.
- Cancellation is exceptional: stable-vs-unstable only, on strong unstable-leg moves
  (`pricing.cancel_policy_stable_vs_unstable`).
- Normal rotation is expiry-driven, not cancel/repost churn.
- Offer files are Bech32m `offer1...` strings; Rust validates structure before Dexie post.
- Reconciliation prefers Coinset tx-signal evidence over venue-status heuristics.

## Delivery constraints

- Python 3.11+ for dev tooling (script lint/type-check).
- Required checks: `ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`
- Rust: `cargo test` in `greenfloor-engine/` (operator config, CLI contracts, and policy parity safety net).
- Local gate: `pre-commit run --all-files` (lint + pyright + formatters; ~5–10s warm with
  `PRE_COMMIT_HOME=.cache/pre-commit`). Run `cargo fmt`, `cargo clippy`, and
  `cargo test --manifest-path greenfloor-engine/Cargo.toml` separately before push — same split as CI.

**Deterministic tests (Rust operator paths):**

- Every conditional dispatch gate in operator policy has branch coverage in
  `greenfloor-engine/` tests.
- Polling and wait loops (daemon cycle, coin-op confirmation, websocket recovery) need
  deterministic timeout/warning tests — inject clocks or mock time; avoid wall-clock-only
  assertions.
- Extract repeated test setup into a named helper when it appears in more than two tests.
- Deterministic test harness runtime stays under 10 minutes wall clock (target under 5).

## Completed milestones

- [x] Native Rust operator binaries (ADR 0013)
- [x] Vault KMS signer path; Cloud Wallet GraphQL removed
- [x] Coinset websocket-first taker/lifecycle signals (H2)
- [x] Coin-op Coinset fee preflight diagnostics (H1)
- [x] Testnet11 G1–G3 proof path (CI `live-testnet-e2e.yml`)
- [x] Mainnet manager lifecycle evidence for `eco1812022_sell_wusdbc`
- [x] Rust-owned operator config policy; Python config field CLI adapters

## Open items

- [ ] **H3:** Evaluate Cloud Wallet native offer split options vs local pre-offer split
      orchestration — preserve denomination readiness guardrails before changing defaults.

## Deferred (post live proof)

- Config editing commands (`set-ladder-entry`, `set-bucket-count`, …)
- Config history, metrics export, coin-op budget reports
- Additional CLI surface without live-target justification

## References

- Deployment: `docs/runbook.md`
- Migration from pre-Rust CLI: `docs/rust-migration-ledger.md`
- Recent work: `docs/progress.md`
