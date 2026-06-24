#!/usr/bin/env bash
# Collect Rust test coverage via llvm-cov.
#
# INCREMENTAL=1  — nextest filter from changed production paths + integration binaries.
# INCREMENTAL=0  — full test suite (local/manual).
#
# CI gates this script with diff-coverage-scope.sh.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

manifest="${CARGO_MANIFEST:?CARGO_MANIFEST is required}"
compare_branch="${COMPARE_BRANCH:-origin/main}"

if [[ "${INCREMENTAL:-0}" == "1" || "${INCREMENTAL:-0}" == "true" ]]; then
  changed_files="$(
    bash "${script_dir}/changed-production-rust-files.sh" "${compare_branch}"
  )"
  if [[ -z "${changed_files}" ]]; then
    echo "No production Rust changes; skipping coverage collection."
    exit 0
  fi

  if ! filter="$(
    printf '%s\n' "${changed_files}" \
      | bash "${script_dir}/rust-coverage-nextest-filter.sh"
  )"; then
    echo "Production Rust files changed but no nextest filter could be built:" >&2
    printf '%s\n' "${changed_files}" >&2
    exit 1
  fi

  echo "Incremental coverage nextest filterset: ${filter}"
  # -E is a nextest runner flag; do not place it after `--` (that forwards it to test binaries).
  cargo llvm-cov nextest \
    --manifest-path "${manifest}" \
    --features test-support \
    -E "${filter}"
  cargo llvm-cov report \
    --manifest-path "${manifest}" \
    --lcov \
    --output-path lcov.info
else
  cargo llvm-cov nextest --manifest-path "${manifest}" --features test-support
  cargo llvm-cov report --manifest-path "${manifest}" --lcov --output-path lcov.info
fi
