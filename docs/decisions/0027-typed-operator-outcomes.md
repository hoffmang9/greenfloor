# ADR 0027: Typed operator outcomes

## Status

Accepted (2026-08-13).

## Context

Operator control flow flattened typed Coinset, Dexie, and driver failures to strings,
then re-parsed them (retry marker lists, mixed-split normalize, forged Dexie
`success: false` JSON). That hid the real 404 / RPC / transport boundary and made
retryability a substring match.

## Decision

1. **`SignerError` by domain.** Variants wrap `Vault`, `CoinOps`, `Offer`, `Transport`,
   `Persistence`, `Config`, and `Reconcile`. Do not flatten those to strings and
   re-parse them for control flow.
2. **Typed transport.** `TransportError::from_reqwest` maps timeout, connect, decode,
   request, and `HttpStatus`. `TransportError::Coinset(String)` is Coinset RPC
   `success: false` only. Retryability is `is_retryable_upstream()` (variant match).
   Mixed-split unspendable coins map at the driver boundary
   (`VaultError::MixedSplitSelectedCoinsNotSpendable`).
3. **Typed Dexie.** HTTP status and visibility stay `SignerError` until the operator
   JSON edge. `get_offer` has one 404 boundary. `DexieOfferFetch` is `Found`, `Missing`,
   or `Mismatch`; Found requires a nested `offer` whose id matches the request.

## Consequences

- Reconcile Found fixtures nest `{ "offer": { ... } }` with a matching id.
- CLI retry/fallback reads `err.is_retryable_upstream()`, not marker lists.
