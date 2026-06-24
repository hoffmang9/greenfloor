#!/usr/bin/env bash
# Best-effort: download lcov.info from the latest successful main CI run.
set -euo pipefail

output_path="${1:-lcov.info.baseline}"

if ! command -v gh >/dev/null 2>&1; then
  echo "gh not available; skipping baseline coverage download."
  exit 0
fi

run_id="$(
  gh run list \
    --repo "${GITHUB_REPOSITORY}" \
    --workflow=ci.yml \
    --branch=main \
    --status=success \
    --limit=1 \
    --json databaseId \
    --jq '.[0].databaseId' 2>/dev/null || true
)"
if [[ -z "${run_id}" ]] || [[ "${run_id}" == "null" ]]; then
  echo "No successful main CI run found; skipping baseline coverage download."
  exit 0
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

if ! gh run download "${run_id}" \
  --repo "${GITHUB_REPOSITORY}" \
  --name coverage-reports \
  --dir "${tmpdir}" 2>/dev/null; then
  echo "Main CI run ${run_id} has no coverage-reports artifact; skipping baseline download."
  exit 0
fi

if [[ ! -f "${tmpdir}/lcov.info" ]]; then
  echo "coverage-reports artifact missing lcov.info; skipping baseline download."
  exit 0
fi

cp "${tmpdir}/lcov.info" "${output_path}"
echo "Downloaded baseline lcov.info from main CI run ${run_id}."
