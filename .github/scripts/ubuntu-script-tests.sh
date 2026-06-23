#!/usr/bin/env bash
# Run script adapter unit tests on ubuntu-latest (plain or coverage-instrumented).
set -euo pipefail

run_py_cov="${RUN_PY_COV:-false}"

cd scripts
if [[ "${run_py_cov}" == "true" ]]; then
  python -m coverage run -m unittest greenfloor_scripts.test_adapters
  python -m coverage xml -o ../coverage-python.xml
else
  PYTHONPATH=. python -m unittest greenfloor_scripts.test_adapters
fi
