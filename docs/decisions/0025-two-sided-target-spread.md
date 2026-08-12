# ADR 0025: Two-sided target spread around mid

## Status

Accepted (2026-08-12).

## Context

`strategy_target_spread_bps` was parsed and copied onto strategy actions, but offer
build used a single mid (`fixed_quote_per_base` or min/max midpoint) for both sides.
`byc_two_sided_wusdbc` therefore posted bid and ask at the same price (0.999), so
there was no spread and par (1.0) sat above both quotes.

Sell-only books used to set `strategy_target_spread_bps` (historically unused). Applying
it there would move ECO ask prices, so those configs omit the field. Strategy/dispatch
also carried a dead `target_spread_bps` on `PlannedAction` that never reached quote math.

## Decision

1. **Full-width bps, split equally.** `strategy_target_spread_bps` is the bid-ask width.
   Buy = mid × (1 − half); sell = mid × (1 + half).
2. **Two-sided only.** `MarketConfig::quote_price_for_side` is the only policy entry.
   It applies the offset when `mode` is `two_sided`. Sell-only YAML omits the field.
   If it is still present, `quote_price` stays mid so it cannot move posted asks.
3. **One price path.** Create, unique-maker pin, bootstrap denomination, and reservation
   all take the side-adjusted quote from `MarketConfig` so clip mojos stay consistent.
   Strategy actions do not carry spread; `PlannedAction` / `StrategyConfig` no longer
   have `target_spread_bps`.

## Consequences

- `byc_two_sided_wusdbc` mid 1.0 + 20 bps posts bid 0.999 / ask 1.001 (1.0 inside).
- Buy clips change with the bid; existing 9990-mojo wUSDC.b makers still match 20 bps
  at size 10. Sell makers stay 10000-mojo BYC; only the requested quote amount changes.
- Open Direct listings at the old price remain until listing expiry or take.
