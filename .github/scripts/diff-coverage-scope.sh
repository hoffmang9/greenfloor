#!/usr/bin/env bash
# Emit GitHub Actions outputs: whether Rust/Python diff coverage should run.
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

if [[ "${run_rust_cov}" == "true" ]]; then
  echo "run_rust_cov=true" >>"${GITHUB_OUTPUT}"
else
  echo "run_rust_cov=false" >>"${GITHUB_OUTPUT}"
fi

if [[ "${run_py_cov}" == "true" ]]; then
  echo "run_py_cov=true" >>"${GITHUB_OUTPUT}"
else
  echo "run_py_cov=false" >>"${GITHUB_OUTPUT}"
fi
