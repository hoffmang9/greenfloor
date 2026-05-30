# 0003 - Parallel Offer Reservation Coordinator

## Status

Accepted; updated 2026-05-29 (signer-only managed post)

## Decision

Use a reservation coordinator with persistent lease tracking for managed signer offer admission instead of coarse end-to-end thread locks.

- Reservation key scope: `(wallet_id, asset_id)`.
- Admission uses amount-based capacity checks (`available - reserved >= requested`) per required asset.
- Reservation leases are persisted in SQLite and released on terminal offer outcomes.
- Stale leases are reclaimed via TTL expiration during daemon startup/cycle entry.
- Parallel worker dispatch for managed signer offer execution is runtime-gated.

## Rationale

- Per-market parallel dispatch already exists, but offer creation historically waited sequentially and had no cross-market reservation guard.
- Cloud wallet coin selection is server-side; with the signer path, coin listing uses Coinset + vault KMS, but amount-based admission remains the safest client-side mechanism for parallel dispatch.
- Holding coarse locks for full offer lifecycle is too expensive because signature/artifact/venue waits can last minutes.
- Persisted leases reduce risk of orphaned in-memory locks and allow deterministic cleanup/recovery.

## Consequences

- Improved throughput for markets sharing the same runtime cycle while reducing self-contention and duplicate spend attempts.
- New operational controls are required in `runtime`:
  - `offer_parallelism_enabled`,
  - `offer_parallelism_max_workers`,
  - `reservation_ttl_seconds`.
- Reservation correctness now depends on timely lease release and TTL cleanup; monitoring should include reservation-specific audit signals.
- This design coordinates workers within a GreenFloor runtime/process and persisted state DB; it is not a global distributed lock across multiple independent deployments sharing the same vault wallet.
