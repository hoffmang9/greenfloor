# ADR 0015: On-chain offer cancel and reclaim

## Status

Accepted (2026-06-22).

## Context

GreenFloor posts offers as Bech32m `offer1...` strings to Dexie. Dexie does **not**
expose a public cancel/delete API. Withdrawing an offer requires spending the offered
input coin on-chain (typically back to vault change) and letting Dexie observe the spend.

Earlier cancel paths assumed Dexie-side cancellation or replanned presplit bindings at
cancel time (wrong `offer_nonce`), which broke presplit-existing production offers.

## Decision

1. **Cancel is on-chain only.** Dexie is used to fetch the offer file (`GET /v1/offers/:id`).
   Coinset MSP submits the cancel spend bundle (`push_tx` / broadcast helpers in
   `greenfloor-engine/src/coinset/`).

2. **Shared spend construction lives in `offer/reclaim.rs`.**
   - `build_offer_cancel_spend_bundle` — decode offer → resolve input CAT → build reclaim spend.
   - `build_vault_cat_reclaim_spend_bundle` — direct vault vs presplit-offer inner spend modes.
   - Lifecycle orchestration (`offer/lifecycle/cancel.rs`) handles Dexie fetch, Coinset
     broadcast, DB updates, and mempool tx observation.

3. **Presplit cancel binding is extracted from the offer input spend**, not replanned.
   `PresplitOfferBinding::from_presplit_input_spend` parses the maker input coin spend in
   the decoded offer bundle, peels MIPS/OneOfN wrappers, and derives the fixed delegated
   puzzle hash. This matches the hash baked into the coin at offer-build time regardless of
   source-coin nonce used during planning.

3a. **Presplit cancel metadata is persisted at post time** in `offer_state`:
   `presplit_input_coin_id`, `fixed_delegated_puzzle_hash`, and `execution_mode`. Cancel
   prefers stored metadata; when absent (legacy rows, DB loss, manual posts), cancel falls
   back to extraction from the Dexie offer file.

4. **Input CAT resolution uses `OfferCoinsetBackend::fetch_unspent_offer_input_cat`.**
   Lookup tries coin id from the offer spend bundle (authoritative for on-chain coins),
   then optional inner-puzzle + amount fallback when decoded-offer coin ids differ.

5. **Optimistic operator state is `cancel_submitted`, not `cancelled`.**
   Successful cancel **submit** records `cancel_submitted` and observes the cancel tx id
   for reconcile. Reconcile promotes to `cancelled` when Dexie status is `3` or chain
   evidence confirms the spend. Failed submit does **not** mark the offer cancelled.

6. **CLI and audit naming reflects submit semantics.**
   - `greenfloor-manager offers-cancel` JSON: `submitted_count` (successful on-chain submits).
   - Daemon `offer_cancel_policy` success items: `status: "cancel_submitted"`,
     `reason: "cancel_submitted_on_strong_unstable_move"`.
   - **`--offer-id` does not require a state DB row.** Dexie fetch + on-chain cancel still
     runs for owned offers missing from SQLite (optional `--market-id` for post-cancel state).
   - **`--offer-file`** accepts a local path or inline `offer1...` bech32 when Dexie id is
     unknown or unavailable (no Dexie fetch).

7. **Presplit cancel inner spend follows ent-wallet `CONDITIONS_OR_SINGLETON` / `SINGLETON`
   path:** cancel delegated spend at MIPS top level + vault singleton member for
   fast-forward (see `build_presplit_offer_cancel_inner_spend` in `offer/presplit.rs`).

## Consequences

- No `DexieClient::cancel_offer` or Dexie POST cancel endpoints in operator code.
- `--cancel-open` selects all open/pending_visibility rows (paginated), excluding
  `cancel_submitted`.
- Operator JSON consumers must use `submitted_count`; `cancelled_count` is removed.
- Simulator coverage: presplit-existing `build_offer_cancel_spend_bundle` roundtrip test.

## References

- Dexie integration notes: [DEXIE_DOCS_AND_API.md](../DEXIE_DOCS_AND_API.md)
- Operator procedures: [runbook.md](../runbook.md)
- ent-wallet presplit cancel: `../ent-wallet/packages/utils/src/chia/p2.ts` (`P2ConditionsOrSingletonType.SINGLETON`)
