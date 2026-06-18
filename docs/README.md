# Architecture decision records

GreenFloor records non-trivial architecture choices here. **Start with the latest
accepted decision** when onboarding; older ADRs may be superseded.

## Current (operator + engine)

| ADR                                                      | Topic                                                              |
| -------------------------------------------------------- | ------------------------------------------------------------------ |
| [0013](decisions/0013-rust-cli-daemon-native-cutover.md) | **Native Rust CLI/daemon** — production operator path              |
| [0010](decisions/0010-rust-engine-crate-naming.md)       | Crate and module naming (`greenfloor-engine`, `greenfloor_engine`) |
| [0007](decisions/0007-rust-signer-pyo3-boundary.md)      | Signer in Rust; scripts use CLI (PyO3 removed 2026-06-17)          |
| [0001](decisions/0001-architecture-boundaries.md)        | Core vs adapters vs orchestration boundaries                       |

## Superseded (historical)

| ADR                                                              | Superseded by                          |
| ---------------------------------------------------------------- | -------------------------------------- |
| [0011](decisions/0011-offer-request-python-import-boundaries.md) | 0013 (Python offer bridges removed)    |
| [0012](decisions/0012-manager-cli-rust-orchestration-cutover.md) | 0013                                   |
| [0009](decisions/0009-manager-cli-modularization.md)             | 0013                                   |
| [0008](decisions/0008-offer-runtime-modularization.md)           | 0013 (Python offer runtime removed)    |
| [0005](decisions/0005-runtime-composition-root.md)               | 0013 (Python composition root removed) |

## Related docs

- Operator procedures: [runbook.md](runbook.md)
- Coinset script validation: [coinset-validation.md](coinset-validation.md)
- Script config adapters: [../scripts/README.md](../scripts/README.md)
- V1 scope: [plan.md](plan.md)
- Migration catch-up: [rust-migration-ledger.md](rust-migration-ledger.md)
- Recent milestones: [progress.md](progress.md)
