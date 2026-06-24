#!/usr/bin/env bash
# List greenfloor-engine production Rust paths (one repo-relative path per line).
#
# Usage:
#   changed-production-rust-files.sh [compare-branch]
#   git diff --name-only BASE...HEAD | changed-production-rust-files.sh -
set -euo pipefail

compare_branch="${1:-origin/main}"

is_excluded_production_rust_path() {
  local file="$1"
  local local_path="${file#greenfloor-engine/src/}"
  local_path="${local_path%.rs}"

  [[ "${local_path}" == *"/tests/"* ]] \
    || [[ "${local_path}" == test_support/* ]] \
    || [[ "${local_path}" == *"/test_support/"* ]] \
    || [[ "${local_path}" == *"/test_env/"* ]] \
    || [[ "${local_path}" == *"/test_overrides/"* ]] \
    || [[ "${local_path}" == */tests ]] \
    || [[ "${local_path}" == *_tests ]]
}

filter_production_rust_paths() {
  while IFS= read -r file; do
    [[ -n "${file}" ]] || continue
    case "${file}" in
      greenfloor-engine/src/*.rs)
        if is_excluded_production_rust_path "${file}"; then
          continue
        fi
        printf '%s\n' "${file}"
        ;;
    esac
  done
}

if [[ "${compare_branch}" == "-" ]]; then
  filter_production_rust_paths
else
  filter_production_rust_paths < <(git diff --name-only "${compare_branch}"...HEAD)
fi
