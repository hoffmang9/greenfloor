#!/usr/bin/env bash
# Collect Rust test coverage via llvm-cov.
#
# INCREMENTAL=1  — fixed nextest filter + optional BASELINE_LCOV merge for diff-cover.
# INCREMENTAL=0  — full test suite (local/manual).
#
# CI gates this script with diff-coverage-scope.sh; do not invoke INCREMENTAL=1 without
# a production Rust diff unless you only want the filtered test run locally.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

manifest="${CARGO_MANIFEST:?CARGO_MANIFEST is required}"
baseline_lcov="${BASELINE_LCOV:-lcov.info.baseline}"
incremental_nextest_filter='test(/greenfloor_engine/) | test(/offer/) | test(/daemon/)'

if [[ "${INCREMENTAL:-0}" == "1" || "${INCREMENTAL:-0}" == "true" ]]; then
  echo "Incremental coverage nextest filterset: ${incremental_nextest_filter}"
  cargo llvm-cov nextest \
    --manifest-path "${manifest}" \
    --features test-support \
    -- \
    -E "${incremental_nextest_filter}"
  cargo llvm-cov report \
    --manifest-path "${manifest}" \
    --lcov \
    --output-path lcov.info.incremental

  bash "${script_dir}/merge-lcov-reports.sh" \
    "${baseline_lcov}" \
    lcov.info.incremental \
    lcov.info
else
  cargo llvm-cov nextest --manifest-path "${manifest}" --features test-support
  cargo llvm-cov report --manifest-path "${manifest}" --lcov --output-path lcov.info
fi
