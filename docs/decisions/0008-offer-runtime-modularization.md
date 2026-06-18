# 0008 - Offer Runtime Modularization

## Status

**Superseded** by [0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md) (2026-06-17)

## Summary

This ADR described modular Python offer runtime under `greenfloor/runtime/` with
`OfferPostRequest` dispatch shared by CLI and daemon.

That Python orchestration layer is **removed**. Offer build/post now lives in:

- `greenfloor-engine/src/offer/operator/build_and_post/` — manager + daemon
- `greenfloor-engine/src/offer/operator/signer_denomination/` — bootstrap denomination

Do not reintroduce Python offer orchestration modules. See ADR 0013.
