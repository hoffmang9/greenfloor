#!/usr/bin/env bash
# Build instrumented test artifacts into CARGO_TARGET_DIR without running tests.
# Used on main pushes so the `coverage` rust-cache is saved under refs/heads/main.
#
# Do not use `cargo llvm-cov … --no-run`: that flag means "report without running"
# (deprecated), not nextest's compile-only mode.
set -euo pipefail

manifest="${CARGO_MANIFEST:?CARGO_MANIFEST is required}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="${repo_root}/greenfloor-engine/target-coverage"
fi

engine_dir="$(cd "$(dirname "${manifest}")" && pwd)"
echo "Warming instrumented coverage artifacts in ${CARGO_TARGET_DIR} (show-env + nextest --no-run)."
(
  cd "${engine_dir}"
  eval "$(cargo llvm-cov show-env --sh)"
  cargo nextest run --features test --no-run
)
