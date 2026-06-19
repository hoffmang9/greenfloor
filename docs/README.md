# Architecture decision records

GreenFloor records non-trivial architecture choices here. **Start with the latest
accepted decision** when onboarding.

## Current (operator + engine)

| ADR                                                              | Topic                                                              |
| ---------------------------------------------------------------- | ------------------------------------------------------------------ |
| [0013](decisions/0013-rust-cli-daemon-native-cutover.md)         | **Native Rust CLI/daemon** — production operator path              |
| [0010](decisions/0010-rust-engine-crate-naming.md)               | Crate and module naming (`greenfloor-engine`, `greenfloor_engine`) |
| [0007](decisions/0007-rust-signer-and-coinset-io.md)             | Vault KMS signing and Coinset IO in Rust                           |
| [0003](decisions/0003-parallel-offer-reservation-coordinator.md) | Parallel managed-post reservation leases                           |

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

- [Cloud Wallet API](CLOUD_WALLET_DOCS_AND_API.md)
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
