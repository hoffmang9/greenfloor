#!/usr/bin/env bash
# Collect Rust test coverage via llvm-cov (requires coverage cache namespace).
set -euo pipefail

manifest="${CARGO_MANIFEST:?CARGO_MANIFEST is required}"

cargo llvm-cov nextest --manifest-path "${manifest}" --features test-support
cargo llvm-cov report --manifest-path "${manifest}" --lcov --output-path lcov.info
