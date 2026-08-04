# ADR 0021: Three ownership simplifications (expired maker, reconcile prep, shape)

## Status

Accepted (2026-08-03).

## Context

After [#177](https://github.com/Chia-Network/greenfloor/pull/177) unified vault mixed-split
*submit* and moved market-cycle reconcile apply into `offer::lifecycle::market_reconcile`,
three residual ownership forks remained:

1. Soft-expire surplus reclaim planning lived in `daemon/` while mark/lease lived in
   `offer/lifecycle/` (ADR 0020 complexity).
2. Market reconcile prepare/heal/watch helpers were private to `market_reconcile/watch_plan`
   while CLI `reconcile_watched_offers` duplicated Dexie `get_offer` match arms.
3. Bootstrap and managed auto-split still forked *planning* (combine-first funding, ladder
   deficits) despite sharing submit via `CatSelection`.

## Decision

Behavior-preserving ownership moves only (no policy changes).

1. **Expired / surplus maker spine** — `offer::lifecycle::expired_maker` owns soft-mark,
   CAS lease, surplus reclaim plan (`plan_soft_expire_reclaims`), and
   `reclaim_expired_maker_if_unspent`. Daemon `soft_expire_phase` stays a thin adapter
   (stable-only gate, capacity counts, soft-fail reclaim loop). Cancel (ADR 0015) remains
   a separate state machine and only shares spend construction in `offer::reclaim`.

2. **Reconcile prepare/heal share** — `offer::lifecycle::reconcile_prep` owns local prepare,
   metadata heal, Dexie fetch parsing (`DexieOfferFetch`), and
   `fetch_and_apply_watched_offer`. Market cycle keeps list `get_offers`, metrics, heal-only,
   and cancel-orphan prep. **CLI `offers-reconcile` stays behavior-preserving:** per-id
   `get_offer` via shared fetch+apply; it does **not** gain local heal or cancel-orphan prep.

3. **Shape planning core** — `coin_ops::shape` owns `CombineInputs`, deficit collection,
   ladder-preserving combine, and funding resolve (`plan_shape_from_deficits` /
   `resolve_shape_funding`) with explicit `AmountUnit` and selector options. Bootstrap
   planner and managed auto-split become thin wrappers. `plan_coin_ops` remains the batch
   count/fee scheduler. Dust lineage batching stays out of ladder shape.

## Consequences

- Soft-expire and ensure PreferExisting share one lease + reclaim spine under lifecycle.
- Reconcile HTTP paths share Dexie fetch/apply helpers without forcing CLI/daemon parity.
- Bootstrap vs daemon funding policy stays explicit (smallest non-cannibalizing vs largest;
   ladder protect flag) rather than silently merging selectors.
- Daemon remains phase orchestrator (not a thin shell); path-specific gates, fee budgets,
  and dust Preselected submit stay path-local.

## Non-goals

- Fold cancel into expired-maker CAS.
- CLI heal / cancel-orphan parity with market reconcile.
- Replace `plan_coin_ops` with one-shot `ShapePlan`.
- Fold dust `plan_dust_*` into ladder shape.
