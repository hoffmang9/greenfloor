# Architecture decision records

GreenFloor records non-trivial architecture choices here. **Start with the latest
accepted decision** when onboarding.

## Current (operator + engine)

| ADR                                                                 | Topic                                                                       |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| [0027](decisions/0027-typed-operator-outcomes.md)                   | **Typed outcomes** — domain `SignerError`; no flatten-then-reparse          |
| [0026](decisions/0026-combine-dust-remainder-coin.md)               | **Combine** — dust remainder on covering shape; exact-denom managed/CLI     |
| [0025](decisions/0025-two-sided-target-spread.md)                   | **Two-sided spread** — bid/ask around mid; sell-only omits the field        |
| [0024](decisions/0024-coin-ops-effective-counts-and-buffer-raid.md) | **Effective counts** — free inventory vs makers; buffer-raid splits         |
| [0023](decisions/0023-canonical-cat-outer-puzzle-hash.md)           | **CAT outer hash** — one `coinset/cats/outer` primitive + Coinset hex       |
| [0022](decisions/0022-unique-maker-coins.md)                        | **Unique Direct makers** — one exact-size receive CAT per open Direct offer |
| [0021](decisions/0021-three-ownership-simplifications.md)           | **Ownership spines** — expired maker, reconcile prep, `coin_ops::shape`     |
| [0020](decisions/0020-soft-expiry-stable-makers.md)                 | **Soft listing expiry** — stable makers, `ensure_size_n_offer`              |
| [0019](decisions/0019-coinset-ws-local-watches.md)                  | **Local watches** — Coinset WS p2/coin-id; default publish venue `coinset`  |
| [0018](decisions/0018-coinset-parse-decomposition.md)               | **Coinset submodule layout** — parse, pagination, rpc_result, json_util     |
| [0017](decisions/0017-offer-submodule-decompositions.md)            | **Offer submodule layout** — bootstrap planner/phase + presplit             |
| [0016](decisions/0016-sqlite-persistence-coverage-policy.md)        | **SQLite coverage** — exclude `storage/sqlite/` from llvm-cov/diff-cover    |
| [0015](decisions/0015-on-chain-offer-cancel.md)                     | **On-chain offer cancel** — reclaim spend, `cancel_submitted` lifecycle     |
| [0014](decisions/0014-offer-publish-module-decomposition.md)        | **Offer publish decomposition** — bootstrap gate + publish assets           |
| [0013](decisions/0013-rust-cli-daemon-native-cutover.md)            | **Native Rust CLI/daemon** — production operator path                       |
| [0010](decisions/0010-rust-engine-crate-naming.md)                  | Crate and module naming (`greenfloor-engine`, `greenfloor_engine`)          |
| [0007](decisions/0007-rust-signer-and-coinset-io.md)                | Vault KMS signing and Coinset IO in Rust                                    |
| [0003](decisions/0003-parallel-offer-reservation-coordinator.md)    | Parallel managed-post reservation leases                                    |

## Superseded ADRs (removed from tree; see git history)

These records were folded into the current operator/engine cutover or are no longer
actionable. Use `git log -- docs/decisions/<file>` to read the original text.

| Former ADR | Topic (short)                          | Superseded by / rationale                                                               |
| ---------- | -------------------------------------- | --------------------------------------------------------------------------------------- |
| 0001       | Architecture boundaries                | [0013](decisions/0013-rust-cli-daemon-native-cutover.md), `AGENTS.md`                   |
| 0002       | Signing pipeline consolidation         | [0007](decisions/0007-rust-signer-and-coinset-io.md)                                    |
| 0004       | Subprocess override threat model       | Native Rust operator path ([0013](decisions/0013-rust-cli-daemon-native-cutover.md))    |
| 0005       | Runtime composition root               | [0013](decisions/0013-rust-cli-daemon-native-cutover.md)                                |
| 0006       | Rust signer canonical path             | [0007](decisions/0007-rust-signer-and-coinset-io.md)                                    |
| 0008       | Offer runtime modularization           | In-crate `greenfloor-engine/src/offer/` modules                                         |
| 0009       | Manager CLI modularization             | In-crate `greenfloor-engine/src/manager_cli/` modules                                   |
| 0011       | Offer request Python import boundaries | Python orchestration removed ([0013](decisions/0013-rust-cli-daemon-native-cutover.md)) |
| 0012       | Manager CLI Rust orchestration cutover | [0013](decisions/0013-rust-cli-daemon-native-cutover.md)                                |

## Integration references

- [Cloud Wallet API](CLOUD_WALLET_DOCS_AND_API.md) — external ent-wallet GraphQL reference (not operator runtime)
- [Coinset API](COINSET_DOCS_AND_API.md)
- [Dexie API](DEXIE_DOCS_AND_API.md)
- [Splash offer submission](SPLASH_OFFER_SUBMISSION_GUIDE.md)

## Related docs

- Operator procedures: [runbook.md](runbook.md)
- Coinset script validation: [coinset-validation.md](coinset-validation.md)
- Script config adapters: [../scripts/README.md](../scripts/README.md)
- V1 scope: [plan.md](plan.md)
- Migration catch-up: [rust-migration-ledger.md](rust-migration-ledger.md)
- Recent milestones: [progress.md](progress.md)
