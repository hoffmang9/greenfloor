#!/usr/bin/env bash
# Emit GitHub Actions outputs for diff-coverage and coverage-cache planning.
#
# Outputs:
#   run_rust_cov / run_py_cov — whether to collect + gate diff coverage
#   need_coverage_cache — restore/save llvm-cov target-coverage cache
#   seed_main_coverage — main push with no rust gate: warm instrumented artifacts
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
compare_branch="${1:-origin/main}"
run_rust_cov=false
run_py_cov=false

changed_files="$(git diff --name-only "${compare_branch}"...HEAD)"

if changed_rust="$(
  printf '%s\n' "${changed_files}" \
    | bash "${script_dir}/changed-production-rust-files.sh" -
)" && [[ -n "${changed_rust}" ]]; then
  run_rust_cov=true
fi

while IFS= read -r file; do
  [[ -n "${file}" ]] || continue
  case "${file}" in
    scripts/*.py | scripts/**/*.py)
      run_py_cov=true
      ;;
  esac
done <<<"${changed_files}"

# GitHub scopes Actions caches by ref: PR coverage caches are invisible to other
# PRs. Seed instrumented artifacts on main so new PRs can restore them.
need_coverage_cache=false
seed_main_coverage=false
if [[ "${run_rust_cov}" == "true" ]]; then
  need_coverage_cache=true
fi
if [[ "${GITHUB_REF:-}" == "refs/heads/main" ]]; then
  need_coverage_cache=true
  if [[ "${run_rust_cov}" != "true" ]]; then
    seed_main_coverage=true
  fi
fi

{
  echo "run_rust_cov=${run_rust_cov}"
  echo "run_py_cov=${run_py_cov}"
  echo "need_coverage_cache=${need_coverage_cache}"
  echo "seed_main_coverage=${seed_main_coverage}"
} >>"${GITHUB_OUTPUT}"
