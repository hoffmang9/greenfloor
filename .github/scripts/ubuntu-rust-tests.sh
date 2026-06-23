#!/usr/bin/env bash
# Run the full greenfloor-engine test suite on ubuntu-latest (plain nextest).
set -euo pipefail

manifest="${CARGO_MANIFEST:?CARGO_MANIFEST is required}"

cargo nextest run --manifest-path "${manifest}" --features test-support
