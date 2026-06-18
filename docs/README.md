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
