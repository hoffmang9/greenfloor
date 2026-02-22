# 0002 - Signing Pipeline Consolidation

## Status

Accepted

## Decision

Collapse the active default signing path to a maximum of 3-4 layers:

1. `wallet_executor` (source routing)
2. `chia_keys_executor` (signer-selection validation + broadcast)
3. `chia_keys_signer_backend` (coin discovery/selection + tx output planning)
4. `chia_keys_raw_engine_sign_impl_sdk_submit` (spend-bundle build/sign via `chia-wallet-sdk`)

Older intermediate wrappers remain in-repo for compatibility and migration safety, but they are no longer on the default execution path.

## Rationale

- The previous chain had too many subprocess boundaries, which increased failure surfaces and made reason propagation/debugging expensive.
- Several layers provided mostly pass-through behavior rather than distinct policy or IO boundaries.
- Consolidation preserves the same deterministic contracts while reducing process hops and simplifying operator override points.
- Keeping `chia-wallet-sdk` at the signing edge aligns with the project baseline for sync/sign/offer generation and avoids reintroducing alternate stacks.

## Consequences

- Default signer execution is easier to test and reason about.
- Fewer environment-variable override points are required for normal operation.
- Future signing work should extend existing layers instead of adding new default chain stages without a new architecture decision.
- Coin lookup, chain-history reads, and transaction submission are handled via the Coinset adapter boundary (`greenfloor/adapters/coinset.py`) rather than direct SDK RPC client calls in signing/wallet paths.
