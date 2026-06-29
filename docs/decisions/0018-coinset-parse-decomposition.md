# ADR 0018: Coinset parse and pagination submodule decomposition

## Status

Accepted (2026-06-29)

## Context

`coinset/parse.rs` (~440 lines) mixed JSON payload parsing, typed coin-record helpers,
RPC success checks, pagination field extraction, and generic utilities (`chunk_values`,
`to_coinset_hex`). `pagination.rs` mixed cursor parsing with async page orchestration.
Callers imported from a single `parse` module name that no longer matched its contents.

## Decision

### Coinset layout (`greenfloor-engine/src/coinset/`)

| Module                 | Responsibility                                                                                             |
| ---------------------- | ---------------------------------------------------------------------------------------------------------- |
| `parse/mod.rs`         | Barrel re-exports for JSON → protocol parsing only                                                         |
| `parse/payload.rs`     | `coin_records_from_payload`, `record_from_payload`                                                         |
| `parse/record.rs`      | `coin_id_from_record`, `coin_from_record`, `coin_spend_from_solution_payload`                              |
| `parse/tests.rs`       | Parse unit tests                                                                                           |
| `rpc_result.rs`        | `ensure_coinset_success` (typed SDK responses), `ensure_coinset_rpc_success` (JSON payloads)               |
| `pagination/mod.rs`    | Async cursor page orchestration (`fetch_all_coinset_pages`, endpoint wrappers)                             |
| `pagination/cursor.rs` | `CoinsetRecordsPagination`, `pagination_from_*`, `ensure_complete_page`, `coin_records_page_from_response` |
| `pagination/tests.rs`  | Pagination unit tests                                                                                      |
| `json_util.rs`         | Coinset JSON scan helpers: `to_coinset_hex`, `u64_from_value`                                              |
| `batch.rs`             | Generic `chunk_values` batching for scan/lineage queries                                                   |

**Ownership split:**

- **`rpc_result`** owns Coinset RPC success/failure mapping for typed and JSON responses.
- **`parse`** owns JSON coin-record and spend decoding only.
- **`pagination`** owns cursor pagination types, page parsing, and multi-page fetch loops.
- **`json_util`** is the canonical in-crate `0x`-prefixed hex helper for Coinset IO (`wallet_io`, scan paths).

Unspent typed `CoinRecord` filtering (`!record.spent`) is inlined at call sites in `xch`,
`coin_select`, and `cats/list` — a one-line filter does not warrant a shared module.

**Public API:** `greenfloor_engine::coinset::*` re-exports are unchanged (`chunk_values`,
`to_coinset_hex`, `u64_from_value`, `ensure_coinset_rpc_success`, parse fns).

**Behavior note:** `coin_spend_from_solution_payload` uses `hex_to_bytes` (normalize + strip
non-hex) instead of raw `hex::decode(trim_start_matches("0x"))`; tests document the broader
acceptance of prefixed/mixed-case hex.

## Consequences

- Import `ensure_coinset_success` from `coinset::rpc_result` (crate-internal), not `parse`.
- Import pagination helpers from `coinset::pagination`, not `parse`.
- Prefer `json_util::to_coinset_hex` over local `format!("0x{}", hex::encode(...))` in `coinset/`.
- Historical references to monolithic `parse.rs` / `pagination.rs` are superseded by this ADR.

## References

- [0007](0007-rust-signer-and-coinset-io.md) — Rust Coinset IO baseline
- [0017](0017-offer-submodule-decompositions.md) — prior submodule decomposition pattern
