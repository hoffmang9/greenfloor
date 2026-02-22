# Coinset Docs & API Reference

This file summarizes the public docs currently available from `https://www.coinset.org/docs` and linked usage pages.

**As of:** 2026-02-19

## Overview

- Coinset positions itself as a free, fast, reliable Chia blockchain API service.
- Main API base URL in examples: `https://api.coinset.org`.
- Most documented endpoints use `POST` + JSON body.
- Real-time updates are documented via WebSocket at `wss://api.coinset.org/ws`.

## Docs Structure (Observed)

- `Introduction`: `https://www.coinset.org/docs`
- Endpoint categories observed:
  - `usage/blocks`
  - `usage/coins`
  - `usage/fees`
  - `usage/full--node`
  - `usage/mempool`
  - `usage/web-socket`

## API Conventions

- Typical response envelope:
  - `success` (boolean)
  - `error` (string)
  - plus endpoint-specific payload keys
- Hash-like fields are usually documented as hex strings (for example `header_hash`, `coin_id`, `tx_id`).
- Common optional filters on coin queries:
  - `start_height` (`uint32`)
  - `end_height` (`uint32`)
  - `include_spent_coins` (boolean)

## Endpoint Catalog (Verified)

### Blocks

- `POST /get_additions_and_removals` - block additions/removals by `header_hash`
- `POST /get_block` - full block by `header_hash`
- `POST /get_block_count_metrics` - aggregate block metrics (`compact_blocks`, `uncompact_blocks`, `hint_count`)
- `POST /get_block_record` - block record by `header_hash`
- `POST /get_block_record_by_height` - block record by `height`
- `POST /get_block_records` - block records in `[start, end]`
- `POST /get_block_spends` - spends in block by `header_hash`
- `POST /get_block_spends_with_conditions` - spends + conditions by `header_hash`
- `POST /get_blocks` - blocks in `[start, end]` with optional `exclude_header_hash`, `exclude_reorged`
- `POST /get_unfinished_block_headers` - unfinished block headers (empty body)

### Coins

- `POST /get_coin_record_by_name` - one coin record by coin `name`
- `POST /get_coin_records_by_hint` - coin records by single `hint`
- `POST /get_coin_records_by_hints` - coin records by multiple `hints`
- `POST /get_coin_records_by_names` - coin records by multiple coin `names`
- `POST /get_coin_records_by_parent_ids` - coin records by `parent_ids`
- `POST /get_coin_records_by_puzzle_hash` - coin records by one `puzzle_hash`
- `POST /get_coin_records_by_puzzle_hashes` - coin records by `puzzle_hashes`
- `POST /get_memos_by_coin_name` - memos for coin `name`
- `POST /get_puzzle_and_solution` - coin solution by `coin_id` and optional `height`
- `POST /get_puzzle_and_solution_with_conditions` - coin solution + conditions
- `POST /push_tx` - submit a `spend_bundle` to mempool/blockchain

### Fees

- `POST /get_fee_estimate` - fee estimation for `spend_bundle` (supports `target_times`, `spend_count`)

### Full Node

- `POST /get_aggsig_additional_data` - network AGG_SIG additional data
- `POST /get_network_info` - network metadata (`network_name`, prefix, genesis challenge)
- `POST /get_blockchain_state` - full node/blockchain summary state
- `POST /get_network_space` - estimated network space between two block header hashes

### Mempool

- `POST /get_all_mempool_items` - all mempool items
- `POST /get_all_mempool_tx_ids` - all mempool tx ids (spend bundle hashes)
- `POST /get_mempool_item_by_tx_id` - one mempool item by `tx_id`
- `POST /get_mempool_items_by_coin_name` - mempool items by `coin_name`

### WebSocket

- `GET /ws` - realtime stream for new transactions, mempool items, and offer files

## Integration Cheat Sheet (Required Body Fields)

Use this as a quick "minimum payload" guide when wiring clients.

### Blocks

| Endpoint                                 | Required body fields |
| ---------------------------------------- | -------------------- |
| `POST /get_additions_and_removals`       | `header_hash`        |
| `POST /get_block`                        | `header_hash`        |
| `POST /get_block_count_metrics`          | none (`{}`)          |
| `POST /get_block_record`                 | `header_hash`        |
| `POST /get_block_record_by_height`       | `height`             |
| `POST /get_block_records`                | `start`, `end`       |
| `POST /get_block_spends`                 | `header_hash`        |
| `POST /get_block_spends_with_conditions` | `header_hash`        |
| `POST /get_blocks`                       | `start`, `end`       |
| `POST /get_unfinished_block_headers`     | none (`{}`)          |

### Coins

| Endpoint                                        | Required body fields |
| ----------------------------------------------- | -------------------- |
| `POST /get_coin_record_by_name`                 | `name`               |
| `POST /get_coin_records_by_hint`                | `hint`               |
| `POST /get_coin_records_by_hints`               | `hints`              |
| `POST /get_coin_records_by_names`               | `names`              |
| `POST /get_coin_records_by_parent_ids`          | `parent_ids`         |
| `POST /get_coin_records_by_puzzle_hash`         | `puzzle_hash`        |
| `POST /get_coin_records_by_puzzle_hashes`       | `puzzle_hashes`      |
| `POST /get_memos_by_coin_name`                  | `name`               |
| `POST /get_puzzle_and_solution`                 | `coin_id`            |
| `POST /get_puzzle_and_solution_with_conditions` | `coin_id`            |
| `POST /push_tx`                                 | `spend_bundle`       |

### Fees / Full Node / Mempool / WebSocket

| Endpoint                               | Required body fields                                 |
| -------------------------------------- | ---------------------------------------------------- |
| `POST /get_fee_estimate`               | `spend_bundle`                                       |
| `POST /get_aggsig_additional_data`     | none (`{}`)                                          |
| `POST /get_network_info`               | none (`{}`)                                          |
| `POST /get_blockchain_state`           | none (`{}`)                                          |
| `POST /get_network_space`              | `newer_block_header_hash`, `older_block_header_hash` |
| `POST /get_all_mempool_items`          | none (`{}`)                                          |
| `POST /get_all_mempool_tx_ids`         | none (`{}`)                                          |
| `POST /get_mempool_item_by_tx_id`      | `tx_id`                                              |
| `POST /get_mempool_items_by_coin_name` | `coin_name`                                          |
| `GET /ws`                              | none (upgrade to WebSocket)                          |

## Common Pitfalls

- Prefer explicit `Content-Type: application/json` on all `POST` calls.
- Send `{}` for endpoints with empty request bodies instead of omitting the body.
- Treat hash-like fields as hex strings exactly as documented (for example `0x...` values).
- `push_tx` appears in multiple docs paths, but the route is the same API call (`POST /push_tx`).
- Coin query defaults may return only unspent records unless `include_spent_coins` is set.

## Quick Request Patterns

```bash
curl -X POST "https://api.coinset.org/<endpoint>" \
  -H "Content-Type: application/json" \
  -d '<json body>'
```

```bash
wscat -c wss://api.coinset.org/ws
```

## Notes

- Some docs links point to additional pages not yet verified in this file (for example, several "Previous/Next" links outside this fetched set).
- The docs mention a CLI utility on the intro page, but a dedicated setup guide was not discovered in the fetched pages.

## Source Pages

- https://www.coinset.org/docs
- https://coinset.org/
- https://www.coinset.org/docs/usage/blocks/get_additions_and_removals
- https://www.coinset.org/docs/usage/blocks/get_block
- https://www.coinset.org/docs/usage/blocks/get_block_count_metrics
- https://www.coinset.org/docs/usage/blocks/get_block_record
- https://www.coinset.org/docs/usage/blocks/get_block_record_by_height
- https://www.coinset.org/docs/usage/blocks/get_block_records
- https://www.coinset.org/docs/usage/blocks/get_block_spends
- https://www.coinset.org/docs/usage/blocks/get_block_spends_with_conditions
- https://www.coinset.org/docs/usage/blocks/get_blocks
- https://www.coinset.org/docs/usage/blocks/get_unfinished_block_headers
- https://www.coinset.org/docs/usage/coins/get_coin_record_by_name
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_hint
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_hints
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_names
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_parent_ids
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_puzzle_hash
- https://www.coinset.org/docs/usage/coins/get_coin_records_by_puzzle_hashes
- https://www.coinset.org/docs/usage/coins/get_memos_by_coin_name
- https://www.coinset.org/docs/usage/coins/get_puzzle_and_solution
- https://www.coinset.org/docs/usage/coins/get_puzzle_and_solution_with_conditions
- https://www.coinset.org/docs/usage/coins/push_tx
- https://www.coinset.org/docs/usage/fees/get_fee_estimate
- https://www.coinset.org/docs/usage/full--node/full_node_get_aggsig_additional_data
- https://www.coinset.org/docs/usage/full--node/full_node_get_network_info
- https://www.coinset.org/docs/usage/full--node/get_blockchain_state
- https://www.coinset.org/docs/usage/full--node/get_network_space
- https://www.coinset.org/docs/usage/full--node/push_tx
- https://www.coinset.org/docs/usage/mempool/get_all_mempool_items
- https://www.coinset.org/docs/usage/mempool/get_all_mempool_tx_ids
- https://www.coinset.org/docs/usage/mempool/get_mempool_item_by_tx_id
- https://www.coinset.org/docs/usage/mempool/get_mempool_items_by_coin_name
- https://www.coinset.org/docs/usage/web-socket/websocket_connect
