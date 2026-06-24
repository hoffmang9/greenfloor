#!/usr/bin/env bash
# Run script adapter unit tests on ubuntu-latest (plain or coverage-instrumented).
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
run_py_cov="${RUN_PY_COV:-false}"

bash "${script_dir}/test/rust-coverage-scripts_test.sh"

cd scripts
if [[ "${run_py_cov}" == "true" ]]; then
  python -m coverage run -m unittest greenfloor_scripts.test_adapters
  python -m coverage xml -o ../coverage-python.xml
else
  PYTHONPATH=. python -m unittest greenfloor_scripts.test_adapters
fi
