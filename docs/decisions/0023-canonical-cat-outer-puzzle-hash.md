# ADR 0023: Canonical CAT outer puzzle-hash module

## Status

Accepted (2026-08-10).

## Context

`cat(asset_id, p2)` outer puzzle hashes were computed with the same
`CatArgs::curry_tree_hash` formula in four places:

- `vault/cat_create.rs` (`receive_cat_outer_puzzle_hash`)
- `vault_coinset_scan/cat_outer.rs` (Bytes32 + hex adapters)
- `coinset/wallet_io.rs` (`cat_outer_puzzle_hash_hex`)
- `coinset/cats/list.rs` (unspent CAT listing)

Callers learned “how to curry,” not “give me the outer.” Hex format differences
justified a thin Coinset adapter; the hash formula did not.

## Decision

One deep module owns the Bytes32 primitive:

| Module                  | Responsibility                                                                     |
| ----------------------- | ---------------------------------------------------------------------------------- |
| `coinset/cats/outer.rs` | `cat_outer_puzzle_hash(asset, p2) → Bytes32` once; `cat_outer_coinset_hex` adapter |

**Ownership split:**

- **`coinset/cats/outer`** owns the curry formula and the Coinset `0x` hex adapter.
- **`vault/cat_create`** keeps double-wrap assert policy and calls `cat_outer_puzzle_hash`
  directly (no alias).
- **`wallet_io` / `cats/list` / vault scan** adapt inputs (address decode, Coinset queries)
  and call the shared primitive — they do not re-curry.
- Bare-hex compare sites use `normalize_hex_id` on the Coinset form; there is no second
  outer-hash helper.

`vault_coinset_scan/cat_outer.rs` is deleted; scan paths import
`crate::coinset::{cat_outer_coinset_hex, cat_outer_puzzle_hash}`.

## Consequences

- Outer-hash bugs and CAT puzzle upgrades localize to one module.
- Create / scan / WS inventory / list share one `(asset, p2)` interface.
- No behavior change: adapters preserve `0x` Coinset hex; compare paths normalize that form.

## References

- Architecture review 2026-08-10 (Canonical CAT outer puzzle-hash module)
- [0018](0018-coinset-parse-decomposition.md) — Coinset submodule ownership pattern
