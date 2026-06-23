#!/usr/bin/env bash
# Run pre-commit lint/format hooks in parallel (cargo-clippy is separate).
# Keep hooks in sync with .pre-commit-config.yaml (exclude cargo-clippy).
set -euo pipefail

hooks=(
  ruff
  ruff-format
  pyright
  yamllint
  prettier
  audit-event-direct-calls
  cargo-fmt
)

pids=()
for hook in "${hooks[@]}"; do
  pre-commit run "${hook}" --all-files &
  pids+=($!)
done

status=0
for pid in "${pids[@]}"; do
  wait "${pid}" || status=1
done
exit "${status}"
