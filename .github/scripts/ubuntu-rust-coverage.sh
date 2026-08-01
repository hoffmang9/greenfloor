#!/usr/bin/env bash
# Collect Rust test coverage via llvm-cov.
#
# INCREMENTAL=1  — nextest filter from changed production paths + integration binaries.
# INCREMENTAL=0  — full test suite (local/manual).
# WARM_ONLY=1    — instrumented build of test artifacts, no test run / no lcov
#                  (main cache seed; pair with plain nextest on `target/`).
#
# CI sets CARGO_TARGET_DIR to greenfloor-engine/target-coverage so llvm-cov does not
# share a target dir with clippy/plain nextest. Coverage caches must live on
# refs/heads/main for PRs to restore them.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"

manifest="${CARGO_MANIFEST:?CARGO_MANIFEST is required}"
compare_branch="${COMPARE_BRANCH:-origin/main}"
# Keep in sync with `.llvm-cov.toml` → report.ignore-filename-regex.
llvm_cov_ignore_regex='(tests/|test_support/|test_env/|test_overrides|/tests\.rs$|/bin/|/main\.rs$|storage/sqlite/|storage/test_support\.rs$)'

# Default instrumented target dir when unset (CI always sets this explicitly).
if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="${repo_root}/greenfloor-engine/target-coverage"
fi

llvm_cov_nextest() {
  # Optional nextest -E filter as $1; do not place -E after `--`.
  # Pass --no-run as $2 when warming only.
  local -a filter_args=()
  local -a warm_args=()
  if [[ "${#}" -ge 1 && -n "${1}" ]]; then
    filter_args=(-E "${1}")
  fi
  if [[ "${#}" -ge 2 && -n "${2}" ]]; then
    warm_args=("${2}")
  fi
  cargo llvm-cov nextest \
    --manifest-path "${manifest}" \
    --features test \
    --ignore-filename-regex "${llvm_cov_ignore_regex}" \
    "${filter_args[@]}" \
    "${warm_args[@]}"
}

if [[ "${WARM_ONLY:-0}" == "1" || "${WARM_ONLY:-0}" == "true" ]]; then
  echo "Warming instrumented coverage artifacts in ${CARGO_TARGET_DIR} (--no-run)."
  llvm_cov_nextest "" --no-run
  exit 0
fi

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
  llvm_cov_nextest "${filter}"
  cargo llvm-cov report \
    --manifest-path "${manifest}" \
    --ignore-filename-regex "${llvm_cov_ignore_regex}" \
    --lcov \
    --output-path lcov.info
else
  llvm_cov_nextest
  cargo llvm-cov report \
    --manifest-path "${manifest}" \
    --ignore-filename-regex "${llvm_cov_ignore_regex}" \
    --lcov \
    --output-path lcov.info
fi
