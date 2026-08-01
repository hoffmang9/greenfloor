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
   markets (`quote_asset_type: stable`). It CAS soft-expires `open` / `refresh_due` /
   `mempool_observed` rows past `listing_expires_at` (NULL expiry counts as already
   elapsed for legacy rows; concurrent terminal states are not clobbered), then
   groups expired makers by `(side, size)`: if the ladder wants size N and the gap
   (`target_count` minus active ladder capacity) is positive, leave **all** expired
   makers at that size for strategy `ensure_size_n_offer` (hash-match PreferExisting);
   if the gap is zero, reclaim them. Unwanted or missing-size makers are always reclaimed.
   Soft-expire does **not** post. Strategy fills the gap via ensure. Both ensure and
   soft-expire reclaim CAS-claim an expired row into `maker_claimed` with a fencing
   `maker_claim_token` (`ExpiredMakerLease`) before PreferExisting or reclaim I/O;
   restore to `expired` on failure when the coin is still reusable; finalize to
   `cancelled` on success (or when the maker coin is already spent). Restore/finalize/
   renew/stale-sweep CAS on the token so a late worker cannot clobber a newer claim.
   Live workers renew `updated_at` on a heartbeat during PreferExisting/reclaim I/O so
   the stale-claim sweep cannot steal an in-flight lease. Soft-expire capacity uses the
   shared `active_capacity_counts_for_market` watchlist helper as strategy; ladder
   capacity counts `open` / `refresh_due` / in-flight `maker_claimed`, plus recent
   `mempool_observed` (`ReconcileState::counts_toward_ladder_capacity`), via a
   paginated state-filtered query (not a capped newest-first dump). NULL/blank
   `offer_side` uses `effective_offer_side` (default sell) consistently in SQL, plan,
   ensure, and watchlist.
   Unstable (hard on-chain expiry) cleanup stays on the reconcile path — this phase does
   not reclaim those makers.

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
