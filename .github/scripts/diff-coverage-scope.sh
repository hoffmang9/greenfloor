#!/usr/bin/env bash
# Emit GitHub Actions outputs: whether Rust/Python diff coverage should run.
set -euo pipefail

compare_branch="${1:-origin/main}"
run_rust_cov=false
run_py_cov=false

while IFS= read -r file; do
  case "${file}" in
    scripts/*.py | scripts/**/*.py)
      run_py_cov=true
      ;;
    greenfloor-engine/src/*.rs)
      if [[ "${file}" == *"/tests/"* ]] \
        || [[ "${file}" == *"/test_support/"* ]] \
        || [[ "${file}" == *"/test_env/"* ]] \
        || [[ "${file}" == *"/test_overrides/"* ]]; then
        continue
      fi
      run_rust_cov=true
      ;;
  esac
done < <(git diff --name-only "${compare_branch}"...HEAD)

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
