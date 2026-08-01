# ADR 0020: Soft listing expiry for stable-quote maker coins

## Status

Accepted (2026-07-31).

## Context

Operator posts always use `split_input_coins: true`, creating durable
`P2_CONDITIONS_OR_SINGLETON` maker coins. Baking `AssertBeforeSecondsAbsolute` into
fixed CONDITIONS made listing expiry kill the CONDITIONS branch: after Dexie/status
expiry the coin could only be singleton-reclaimed, so re-offering the same size required
a chain move (or left orphaned makers off the receive address).

Stable-quote markets (`quote_asset_type: stable`) want soft takeability — take $X until
the ladder stops wanting that size — with listing rotation that does not move coins when
price is unchanged.

## Decision

1. **Stable soft CONDITIONS.** For `quote_asset_type: stable`, omit
   `AssertBeforeSecondsAbsolute` from presplit fixed CONDITIONS. Listing expiry still
   comes from `strategy_offer_expiry_minutes` and is stored as `listing_expires_at` on
   `offer_state`. Unstable markets keep hard on-chain expiry unchanged.

2. **`ensure_size_n_offer`.** One shared path for daemon post and post-expire reconcile:
   - matching idle/expired maker (planned fixed hash == stored, same nonce) →
     PresplitExisting re-offer (no chain tx);
   - hash mismatch (price/terms change) → reclaim then PresplitNew;
   - no maker → PresplitNew from receive.

3. **Expire handling.** The daemon `soft_expire` phase runs **only** for soft-expiry
   markets (`quote_asset_type: stable`). It soft-expires open rows past
   `listing_expires_at`, then for each expired maker with cancel metadata: if the ladder
   still wants size N, call `ensure_size_n_offer`; else reclaim to vault. Runs before
   strategy so open-count gaps are already filled. Unstable (hard on-chain expiry) cleanup
   stays on the reconcile path — this phase does not ensure/reclaim those makers.

4. **Vault-controlled balance CLI.** `coins-balance` reports receive + known unreturned
   makers (`vault_controlled_amount`). Open makers are listed with `reclaimable: false`.
   Legacy stranded coins without DB metadata remain ops reclaim via
   `offers-reclaim-presplit`.

## Consequences

- Soft expiry is policy, not consensus: anyone with a CONDITIONS spend can still take a
  soft maker until reclaim or take.
- Price changes require reclaim + rebuild (same CONDITIONS cannot carry new terms).
- Persist `listing_expires_at`, `size_base_units`, `offer_nonce`, and `offer_side` at post
  time so expire/repost does not need Dexie status 6 (which will not fire without
  on-chain expiry).

## Related

- [0015](0015-on-chain-offer-cancel.md) — cancel/reclaim spend construction
- [plan.md](../plan.md) — offer policy
- [runbook.md](../runbook.md) — `coins-balance` / `offers-reclaim-presplit`
