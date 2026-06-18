# 0009 - Manager CLI Modularization

## Status

**Superseded** by [0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md) (2026-06-17)

## Summary

This ADR described splitting the monolithic Python `greenfloor/cli/manager.py` into
focused CLI modules with shared Python runtime layers.

That layout is **removed**. Manager commands now live in:

- `greenfloor-engine/src/manager_cli/` — native `greenfloor-manager` binary
- `greenfloor-engine/src/offer/operator/` — shared build/post orchestration
- `greenfloor-engine/src/offer/lifecycle/` — offers status/reconcile/cancel

Do not reintroduce Python CLI orchestration entrypoints. See ADR 0013 and
`docs/rust-migration-ledger.md`.
