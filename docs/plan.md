# GreenFloor V1 Plan

## Scope

- Long-lived daemon (`greenfloord`) plus manager CLI (`greenfloor-manager`) for
  deterministic CAT/XCH market-making.
- Policy and execution in Rust (`greenfloor-engine`); Python limited to `scripts/`
  (vault bootstrap + subprocess adapters to native binaries).
- V1 notifications: low-inventory alerts only (ticker, remaining amount, receive address).

## Architecture

```
Operators                greenfloor-engine (Rust)
─────────                ─────────────────────────
greenfloor-manager  ──►  manager_cli/ → offer/operator, offer/lifecycle, coin_ops/…
greenfloord         ──►  daemon/      → cycle/, offer/operator, coin_ops/execution/…

Dev / scripts            scripts/ (Python adapters)
─────────                ───────────────────────────
vault bootstrap     ──►  create_kms_vault.py → ent-wallet GraphQL (one-time)
adapter unit tests  ──►  greenfloor_scripts/ → engine + manager CLIs
```

- **Canonical signing and offer build:** `greenfloor-engine` (vault KMS + Coinset MSP).
- **Config policy for operators:** Rust (`config/program.rs`, `config/markets.rs`, `config/signer.rs`).
- **Script-facing config reads:** `greenfloor-manager program-fields`, `markets-fields`,
  `cats-fields` (via `greenfloor-manager`); not direct YAML policy walks.
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
8. `offers-cancel` — on-chain cancel by offer id or `--cancel-open` (Dexie fetch + Coinset submit)
9. `coins-list` / `coin-status` — vault coin inventory via Coinset
10. `coin-split` / `coin-combine` — denomination shaping (default waits for confirmation)

Adjunct operator commands:

- `combine-market-cat-dust` — batch merge sub-unit CAT dust for enabled markets
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

### On-chain cancel (ADR 0015)

- Dexie has no public cancel API. Cancel spends an offered vault CAT input coin back to
  vault change; spend construction is shared in `offer/reclaim.rs`.
- `offers-cancel` JSON reports `submitted_count` (successful Coinset submits), not confirmed
  cancels. Failed submits increment `failed_count` only.
- After submit, SQLite state is `cancel_submitted` until reconcile observes Dexie status `3`
  (Cancelled) or chain confirmation; `--cancel-open` skips rows already in
  `cancel_submitted`.
- Presplit-existing offers derive cancel binding from the offer input spend embedded in the
  offer file (fixed delegated puzzle hash), not by replanning with source-coin nonce.
- Daemon cancel audit items use `status: "cancel_submitted"` and
  `reason: "cancel_submitted_on_strong_unstable_move"` on successful submit.

## Delivery constraints

Canonical local/CI gate commands: [README.md](../README.md) → **Local dev tooling** and **Developer Checks**.

- Python 3.11+ for dev tooling (script lint/type-check).
- Node.js LTS for Prettier (YAML/JSON/Markdown); see [README.md](../README.md) → **Local dev tooling**.
- Required checks: `ruff`, `ruff-format`, `prettier`, `yamllint`, `pyright`
- Rust operator tests: `cargo nextest run --manifest-path greenfloor-engine/Cargo.toml` in CI
  (`cargo test` with the same manifest works locally).
- Local gate: `pre-commit run --all-files` (ruff, pyright, prettier, yamllint, cargo fmt/clippy;
  ~5–10s warm with `PRE_COMMIT_HOME=.cache/pre-commit`). Run the Rust test command above
  separately before push — same split as CI.

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
- [x] Rust-owned operator config policy; script config via manager field CLIs
- [x] On-chain offer cancel with `cancel_submitted` lifecycle (ADR 0015)

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
