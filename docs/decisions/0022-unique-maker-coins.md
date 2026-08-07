# ADR 0022: Unique maker coins for Direct offers

## Status

Accepted (2026-08-07).

## Context

On live `byc_two_sided_wusdbc` (ladder: 1 buy + 3 sell × 10):

- **Symptom:** Three open Dexie sells all listed the same maker coin (`0ad7485d…`),
  while the vault held four distinct 10‑BYC receive coins.
- **Cause:** Exact-size CATs take the **Direct** path (no new maker coin). Selection
  picks the first exact unspent match; open makers are not excluded. Parallel dispatch
  only reserves **amounts**, so two workers pick the same coin. A later sequential post
  also re-picks it because Direct leaves the coin unspent.
- **Why it matters:** One take can invalidate sibling listings.
- **Constraint:** Do **not** fix by forcing new off-receive presplit/CONDITIONS makers
  (those disappear from normal Coinset receive-address balance). Stay on vault receive
  CATs.

## Decision

1. **Market flag `unique_maker_coins`** (default **true** when omitted). When true, each
   new Direct offer pins a distinct receive-address CAT via existing create-path
   `offer_coin_ids`. Opt out with `unique_maker_coins: false` for intentional shared
   Direct makers.

2. **Binding excludes** are per `market_id` only: non-empty `cancel_input_coin_id` rows
   whose state satisfies `ReconcileState::binds_unique_maker_coin()` (SQL allowlist
   `BINDING_MAKER_QUERY_STATES`). Terminal states such as `expired` do not bind.

3. **Exact-size pin only.** Pick a free coin with `amount ==` the offered-leg mojo target
   (same multiplier path as create). Never pin oversize coins — that would force a
   split/CONDITIONS maker and violate the receive-address constraint. Fail closed
   (`InsufficientCatCoins` / `NoUnspentCatCoins` family) when no free exact-size coin
   remains. Skip pick when `maker_reuse` is set (PreferExisting).

4. **Single pin site, after bootstrap, with a session exclude set.** Pin runs in each
   `build_and_post` iteration after denomination bootstrap succeeds and before create
   (`needs_live_unique_pin` + `pin_unique_exact_maker_coin_id`), so shaping cannot spend
   the pinned coin. The batch seeds excludes from SQLite bindings (daemon: cycle write
   store; CLI: persist store / home DB). Pinned ids join the in-memory session set only
   after a successful venue publish, so a failed create/publish can reuse the coin on a
   later `repeat` iteration. Dry-run skips Coinset pin. Daemon `ensure_size` reuses that
   path.

5. **Sequential dispatch** when unique: managed parallel dispatch is disabled for that
   market so cross-process binding rows are visible before the next ensure.

## Non-goals

- Changing `select_cats_for_spend` / `OfferCoinsetBackend` signatures
- Forcing PreferExisting / `SplitAndOffer` / new opaque makers
- Cross-market or address-global exclusion
- Editing ADRs 0003 / 0015 / 0019

## Consequences

- Markets with enough exact-size receive coins get distinct `cancel_input_coin_id`s per
  open Direct offer.
- Parallelism for unique markets is sacrificed for correctness; set
  `unique_maker_coins: false` only when shared Direct is intentional.
- Without a free exact-size coin, unique markets refuse the post rather than inventing a
  new CONDITIONS maker off the receive address.
