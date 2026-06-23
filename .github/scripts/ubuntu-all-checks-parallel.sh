#!/usr/bin/env bash
# Lint/format hooks, script adapter tests, and clippy in parallel (non-coverage Rust PRs).
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

pids=()
bash "${script_dir}/ubuntu-lint-parallel.sh" &
pids+=($!)
"${script_dir}/ubuntu-script-tests.sh" &
pids+=($!)
pre-commit run cargo-clippy --all-files &
pids+=($!)

status=0
for pid in "${pids[@]}"; do
  wait "${pid}" || status=1
done
exit "${status}"
