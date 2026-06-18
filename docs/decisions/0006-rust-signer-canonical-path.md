# 0006 - Rust Signer Canonical Path

## Status

Accepted; **migration complete** for operators ([0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md))

## Decision

`greenfloor-engine` is the canonical signing implementation for vault KMS paths.
New vault spend, mixed-split, and offer behavior lands in Rust first.

Python `greenfloor/adapters/rust_signer.py` and related bridges call the engine via
PyO3 for scripts and parity tests only. Operator binaries do not use Python signing.

## Rationale

- Vault MIPS + KMS fast-forward signing is easier to test and keep correct in Rust with
  `chia-wallet-sdk`.
- Single implementation avoids presplit nonce and member-hash drift.

## Consequences

- Feature work for vault CAT spends and offers targets `greenfloor-engine/` first.
- `--split-input-coins` presplit path: when selected CAT inputs already equal offer
  amount exactly, signer uses direct execution (`execution_mode: direct`).

## Supersedes

Partially supersedes ADR 0002 for the implementation layer; adapter boundaries unchanged.
