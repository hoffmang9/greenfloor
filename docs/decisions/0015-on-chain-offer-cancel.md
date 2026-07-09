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

1. **Cancel is on-chain only.** Prefer order for maker-input resolution:
   local `offer1…` text → Coinset coin lookup + stored cancel metadata → optional Dexie
   offer-file fetch (`GET /v1/offers/:id`) for legacy / Dexie-posted rows. Coinset
   `get_offer` intentionally omits the raw offer blob, so it is not used for cancel
   spend construction. Direct Coinset HTTP API submits the cancel spend bundle
   (`push_tx` / broadcast helpers in `greenfloor-engine/src/coinset/`).

2. **Shared spend construction lives in `offer/reclaim.rs`.**
   - `build_offer_cancel_spend_bundle` — decode offer → spend each cancellable maker input
     (vault XCH p2 or presplit CAT) → one vault singleton fast-forward.
   - `build_offer_cancel_spend_bundle_from_metadata` — Coinset coin + stored cancel
     metadata (no offer blob).
   - `build_vault_cat_reclaim_spend_bundle` — direct vault vs presplit-offer inner spend modes.
   - Lifecycle orchestration (`offer/lifecycle/cancel.rs`) handles Coinset-primary cancel,
     optional Dexie offer-file fallback, broadcast, DB updates, and mempool tx observation.

3. **Presplit cancel binding is extracted from the offer input spend**, not replanned.
   `PresplitOfferBinding::from_presplit_input_spend` parses the maker input coin spend in
   the decoded offer bundle, peels MIPS/OneOfN wrappers, and derives the fixed delegated
   puzzle hash. This matches the hash baked into the coin at offer-build time regardless of
   source-coin nonce used during planning.

3a. **Presplit cancel metadata is persisted at post time** in `offer_state`:
`presplit_input_coin_id`, `fixed_delegated_puzzle_hash`, and `execution_mode`. Cancel
prefers Coinset + stored metadata (no offer file). When metadata is absent (legacy rows,
DB loss, manual posts), cancel may fall back to a local `--offer-file` or optional Dexie
offer-file fetch.

4. **Input CAT resolution is coin-id authoritative.** Cancel resolves the offered input
   via stored `presplit_input_coin_id` when present, then scans coin ids from the decoded
   offer spend bundle (offered coin id plus same-amount maker spends). Ambiguous inner-puzzle
   - amount fingerprint lookup is not used — it can select a different vault coin when the
     offer input is already spent.

5. **Optimistic operator state is `cancel_submitted`, not `cancelled`.**
   Successful cancel **submit** atomically records (before `push_tx`):
   - `offer_state.state = cancel_submitted`
   - `cancel_submitted_tx_id` (canonical spend-bundle hash)
   - `cancel_submitted_at` (submit timestamp; preserved across reconcile preserves)
   - clears `offer_coin_watches` for the offer
   - `tx_signal_state` mempool observation for the cancel tx id

   Reconcile promotes to `cancelled` when Dexie status is `3`, cancel tx chain-confirms, or
   other canonical reconcile signals apply. Failed broadcast rolls state back; failed
   persist before broadcast never submits.

6. **`cancel_submitted` reconcile and defer policy.**
   - Pure policy: `cycle/reconcile/cancel_submitted_policy/` (`CancelSubmittedContext`,
     `allowed_cancel_target_offer_ids`, `resolve_cancel_submitted_transition`).
   - SQLite I/O adapter: `offer/lifecycle/cancel_context.rs` (`preload_cancel_submitted_contexts`,
     `cancel_submitted_context_for_offer`, `defer_in_flight_cancel_offer_ids`,
     `partition_defer_in_flight_cancel_targets`).
   - **Partitioned Dexie vs cancel tx signals.** Reconcile classifies only Dexie-linked tx ids
     into mempool/confirmed buckets (`CoinsetTxSignals` / `CoinsetSignalSummary`). Tracked
     cancel tx observation lives in `CancelSubmittedContext` and `chain_confirmed_tx_ids`;
     cancel mempool/confirm never flows through taker dispatch.
   - Orphan grace (`CANCEL_SUBMIT_TRACKING_GRACE_SECS`, default 5 minutes) anchors on
     `cancel_submitted_at`, not `updated_at`, so reconcile preserve upserts do not extend
     the grace window. When `cancel_submitted_at` is missing (legacy rows before migration),
     grace falls back to `tx_signal_state.mempool_observed_at` for the tracked cancel tx id.
   - **In-flight defer is grace-bounded.** Cancel submit is in flight only while the tracked
     cancel tx is unconfirmed **and** still within orphan grace from the anchor timestamp.
     Daemon cancel policy and CLI `--offer-id` / `--cancel-open` defer re-submit only during
     that window.
   - **Confirmed-list semantics.** Reconcile passes the Coinset confirmed tx id list into
     cancel-submitted policy. When the tracked cancel tx id appears in that list, stale
     unwedge (`cancel_submitted` → `open`) is blocked even if Dexie still reports open
     (`status = 1`) and the cancel tx has no `tx_block_confirmed_at` in SQLite yet. Chain
     confirmation still promotes to `cancelled` via the cancel-tx signal path.
   - **Stale unwedge after grace.** When Dexie still reports open (`status = 1`) but the
     cancel tx remains unconfirmed past grace **and** is absent from the confirmed list,
     reconcile resets `cancel_submitted` → `open`
     (`REASON_CANCEL_SUBMIT_STALE_DEXIE_OPEN`). This avoids an indefinite wedge when Coinset
     shows mempool-only observation with no chain confirmation.
   - **Preload fallback.** Batch reconcile preloads cancel-submit context for all
     `cancel_submitted` rows. Per-offer reconcile uses the preloaded map when present; on a
     cache miss it falls through to a row + tx-signal lookup so a single offer is not left
     without context.
   - **Missing context preserves.** When state is `cancel_submitted` but cancel-submit
     context cannot be loaded (no row, lookup failure surfaced as absent context), reconcile
     preserves `cancel_submitted` (`REASON_CANCEL_SUBMIT_CONTEXT_MISSING`) rather than
     applying stale-unwedge with empty defaults.
   - **Persist before broadcast.** Tracked cancels write `cancel_submitted`, clear
     `offer_coin_watches`, and observe the cancel tx id (spend-bundle hash) **before**
     `push_tx`. Broadcast failure rolls state back to the prior lifecycle state and
     restores watches from stored cancel metadata when present. Persist failure before
     broadcast never submits.

7. **CLI and audit naming reflects submit semantics.**
   - `greenfloor-manager offers-cancel` JSON: `submitted_count`, `skipped_count`, and per-item
     `result.skipped` / `result.reason = cancel_submit_in_flight` when defer applies.
   - Daemon `offer_cancel_policy` success items: `status: "cancel_submitted"`,
     `reason: "cancel_submitted_on_strong_unstable_move"`.
   - **`--offer-id` does not require a state DB row.** Dexie fetch + on-chain cancel still
     runs for owned offers missing from SQLite (optional `--market-id` for post-cancel state).
   - **`--offer-file`** accepts a local path or inline `offer1...` bech32 when Dexie id is
     unknown or unavailable (no Dexie fetch).

8. **Presplit cancel inner spend follows ent-wallet `CONDITIONS_OR_SINGLETON` / `SINGLETON`
   path:** cancel delegated spend at MIPS top level + vault singleton member for
   fast-forward (see `build_presplit_offer_cancel_inner_spend` in
   `offer/presplit/conditions.rs`, re-exported from `presplit/mod.rs`).

## Consequences

- No `DexieClient::cancel_offer` or Dexie POST cancel endpoints in operator code.
- `--cancel-open` selects all open/pending_visibility rows (paginated), excluding
  `cancel_submitted`.
- Operator JSON consumers must use `submitted_count`; `cancelled_count` is removed.
- Defer helpers are exported from `offer::lifecycle` (not `cycle::`); reconcile policy
  stays pure and crate-private where possible.
- Simulator coverage: presplit-existing `build_offer_cancel_spend_bundle` roundtrip test.

## References

- Dexie integration notes: [DEXIE_DOCS_AND_API.md](../DEXIE_DOCS_AND_API.md)
- Operator procedures: [runbook.md](../runbook.md)
- Offer module layout: [0017](0017-offer-submodule-decompositions.md)
- ent-wallet presplit cancel: `../ent-wallet/packages/utils/src/chia/p2.ts` (`P2ConditionsOrSingletonType.SINGLETON`)
