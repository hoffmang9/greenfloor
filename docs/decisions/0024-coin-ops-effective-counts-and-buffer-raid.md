# ADR 0024: Coin-ops effective counts and buffer-raid split sources

## Status

Accepted (2026-08-11).

## Context

Inventory bucket scans counted unspent Direct maker coins. `effective_sell_bucket_counts_for_coin_ops`
also credited live sells toward the ladder target. That double-counted open makers, so size-N
coverage looked full (target + buffer) while free inventory could not fund another post.

Separately, low-watermark split protection:

1. Treated `target + buffer` as unsplittable, and
2. Blocked _any_ partial split of an exact ladder clip once `current >= required`, even with
   true excess, and
3. Built protection counts from free (watch-excluded) spendable only — so an open sell’s maker
   did not count toward target coverage for the source row.

On john-deere BYC this left a free size-25 and a fractional 10.3 while the third size-10 sell
failed with `insufficient cat coins`, and coin-ops either planned excess combines or skipped
splits with `no_spendable_split_coin_meets_required_amount`.

## Decision

1. **Free inventory at source** — inventory bucket scans exclude durable maker coin-id watches.
   Freshness cache keys include a watch fingerprint so posts/cancels that do not move coins
   still force a rescan. `effective_sell_bucket_counts_for_coin_ops` then only strips
   same-cycle `newly_executed` clips still present in the pre-strategy snapshot and credits
   `min(active + newly, target)`.
2. **Split-source protection** — `SplitSourceProtection::for_low_watermark_split` (not
   `LadderShapeContext::from_sell_ladder_entries`) builds **target-only** slots via
   `required_ladder_row_slots(..., buffer=0)` over **full-vault** inventory including watched
   makers. Cannibalize iff consuming one exact clip would leave the row below its protected
   count. Split execution fetches inventory once via `list_wallet_coins_for_split`.

## Consequences

- Coin-ops plans size deficits when free inventory is short, even if makers remain unspent.
- A free exact clip above target (including a buffer clip once target is met by open sells)
  can fund a smaller-rung low-watermark split.
- Primary-row clips at exact target coverage remain protected (ECO.181 skip expectations hold).
- Watch-set changes invalidate free-inventory cache without requiring Coinset WS activity.
