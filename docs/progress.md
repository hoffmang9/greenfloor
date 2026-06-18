# Progress Log

Current architecture, live targets, and open work. Pre-Rust migration detail lives in git
history and `docs/rust-migration-ledger.md`.

## Current architecture

**Operators (production):** native Rust binaries only — no Python entrypoints, no PyO3
(ADR 0013).

| Binary               | Role                                                                  |
| -------------------- | --------------------------------------------------------------------- |
| `greenfloor-manager` | Config, keys, cats, coin ops, build/post, offers lifecycle            |
| `greenfloord`        | Market cycle daemon (`--once` or loop)                                |
| `greenfloor-engine`  | Low-level engine CLI (vault debug, `coinset …`, `daemon-once`, tests) |

Policy and execution live in `greenfloor-engine/src/`:

| Module                                     | Responsibility                                               |
| ------------------------------------------ | ------------------------------------------------------------ |
| `config/`                                  | Program, markets, and signer parse/validation                |
| `manager_cli/`                             | Manager command dispatch and JSON output                     |
| `daemon/`                                  | Cycle loop, market phases, Coinset websocket tx signals      |
| `offer/operator/`                          | Shared build/post and signer denomination (manager + daemon) |
| `offer/lifecycle/`                         | Reconcile, cancel, status (manager + daemon)                 |
| `coin_ops/` + `daemon/coin_ops_execution/` | Coin-op policy and execution                                 |
| `cycle/`                                   | Strategy, cancel policy, parallel managed-post dispatch      |
| `coinset/`                                 | Coinset HTTP/MSP IO, fee estimates, script `coinset` CLI     |
| `vault/`                                   | Vault KMS signing and MIPS spend construction                |
| `storage/`                                 | SQLite schema and persistence (`~/.greenfloor/db/…`)         |

**Blockchain access:** Coinset.org for coin lookup, mempool/tx signals, fee estimates, and
tx submission. `chia-wallet-sdk` (repo submodule) is used in Rust for signing, puzzle/offer
construction, and offer validation — not for full-node sync.

**Python (scripts and test harnesses only):** `scripts/` plus a slim `greenfloor/` package
— config field CLI adapters (`greenfloor/config/io.py` → `greenfloor-manager program-fields`,
`markets-fields`, `cats-fields`, `materialize-minimal-program`, `config-validate`), hex
helpers, and Coinset shell-out (`greenfloor.adapters.coinset` → `greenfloor-engine coinset …`).
Scripts must not walk operator YAML for policy fields.

**Quality gates:** `cargo test --manifest-path greenfloor-engine/Cargo.toml` is the operator
policy parity safety net; pytest covers script adapters and subprocess integration harnesses.
Local gate: `pre-commit run --all-files`.

## Shipped (V1 baseline)

- Native Rust operator binaries (`greenfloor-manager`, `greenfloord`)
- Vault KMS signer path; Cloud Wallet GraphQL removed
- Rust-owned operator config policy; Python config field CLI adapters for scripts
- Coinset websocket-first taker/lifecycle signals
- Coin-op Coinset fee preflight diagnostics
- Testnet11 G1–G3 proof path (CI `live-testnet-e2e.yml`)
- Mainnet manager lifecycle evidence for `eco1812022_sell_wusdbc`

## Active live testing

- **Mainnet canary:** `eco1812022_sell_wusdbc` (`ECO.181.2022:wUSDC.b`). See runbook
  §2 mainnet cutover checklist.
- **Testnet11 proof pair:** `TDBX:txch` (CI via `live-testnet-e2e.yml`).

## Open items

- **H3:** Evaluate Cloud Wallet native offer split vs local pre-offer split orchestration —
  preserve denomination readiness guardrails before changing defaults (`docs/plan.md`).

Deferred until live-target justification: config editing commands, metrics export, coin-op
budget reports, and additional CLI surface (`docs/plan.md`).

## Milestones

### 2026-06-17 — Rust-native operator cutover

Single cutover (ADR 0013): native `greenfloor-manager` and `greenfloord`; PyO3 and Python
orchestration removed; Rust owns config policy, signing, offers, coin ops, daemon cycles, and
SQLite. Scripts use `greenfloor-engine coinset …` and manager field CLIs. ADR index trimmed
to active decisions (`0013`, `0010`, `0007`, `0003`).

## References

- V1 scope: `docs/plan.md`
- Operator procedures: `docs/runbook.md`
- Architecture decisions: `docs/README.md` (start with ADR 0013)
- Breaking changes / migration catch-up: `docs/rust-migration-ledger.md`
- Agent policy: `AGENTS.md`
