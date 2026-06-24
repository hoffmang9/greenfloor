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
# Keep in sync with `.llvm-cov.toml` → report.ignore-filename-regex.
llvm_cov_ignore_regex='(tests/|test_support/|test_env/|test_overrides|/tests\.rs$|/bin/|/main\.rs$|storage/sqlite/|storage/test_support\.rs$)'

if [[ "${INCREMENTAL:-0}" == "1" || "${INCREMENTAL:-0}" == "true" ]]; then
  changed_files="$(
    bash "${script_dir}/changed-production-rust-files.sh" "${compare_branch}"
  )"
  if [[ -z "${changed_files}" ]]; then
    echo "No production Rust changes; skipping coverage collection."
    exit 0
  fi

  if ! filter="$(
    {
      printf '%s\n' "${changed_files}"
      git diff --name-only "${compare_branch}"...HEAD \
        | grep -E '^greenfloor-engine/src/test_support/.*\.rs$' || true
    } | bash "${script_dir}/rust-coverage-nextest-filter.sh"
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
    --ignore-filename-regex "${llvm_cov_ignore_regex}" \
    -E "${filter}"
  cargo llvm-cov report \
    --manifest-path "${manifest}" \
    --ignore-filename-regex "${llvm_cov_ignore_regex}" \
    --lcov \
    --output-path lcov.info
else
  cargo llvm-cov nextest \
    --manifest-path "${manifest}" \
    --features test-support \
    --ignore-filename-regex "${llvm_cov_ignore_regex}"
  cargo llvm-cov report \
    --manifest-path "${manifest}" \
    --ignore-filename-regex "${llvm_cov_ignore_regex}" \
    --lcov \
    --output-path lcov.info
fi
