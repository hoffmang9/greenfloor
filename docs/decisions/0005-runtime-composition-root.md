# 0005 - Runtime Composition Root

## Status

**Superseded** by [0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md) (2026-06-17)

## Summary

This ADR named `greenfloor.runtime.offer_execution` as the Python composition root
for offer build/post orchestration.

Operator offer build/post is now **Rust-native** (`offer/operator/`). The Python
`greenfloor/runtime/offer_*` modules are deleted.

For current boundaries see ADR 0013 and `docs/plan.md`.
