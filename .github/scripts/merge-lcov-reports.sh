#!/usr/bin/env bash
# Merge baseline and incremental lcov reports for diff-cover.
set -euo pipefail

baseline="${1:?baseline lcov path}"
incremental="${2:?incremental lcov path}"
output="${3:?output lcov path}"

if [[ ! -f "${baseline}" ]] || ! command -v lcov >/dev/null 2>&1; then
  cp "${incremental}" "${output}"
  exit 0
fi

lcov \
  --add-tracefile "${baseline}" \
  --add-tracefile "${incremental}" \
  --output-file "${output}" \
  --quiet
