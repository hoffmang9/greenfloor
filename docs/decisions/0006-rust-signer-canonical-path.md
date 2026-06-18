# 0006 - Rust Signer Canonical Path

## Status

Accepted; **migration complete** for operators ([0013-rust-cli-daemon-native-cutover.md](0013-rust-cli-daemon-native-cutover.md))

## Decision

`greenfloor-engine` is the canonical signing implementation for vault KMS paths.
New vault spend, mixed-split, and offer behavior lands in Rust first.

Operators and scripts use native Rust binaries only. Python `greenfloor/adapters/`
remains for read-only Coinset HTTP in standalone scripts; mutations call
`greenfloor-engine coinset …` CLI subcommands.

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
