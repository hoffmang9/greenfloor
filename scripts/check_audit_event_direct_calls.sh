#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_src="${root}/greenfloor-engine/src"

violations="$(
  rg '\.add_audit_event(_at)?\(' "${engine_src}" \
    | rg -v 'storage/sqlite/' \
    | rg -v 'operator_log/' \
    | rg -v 'tests\.rs' \
    || true
)"

if [[ -n "${violations}" ]]; then
  echo "direct add_audit_event calls must go through operator_log::operator_audit" >&2
  echo "${violations}" >&2
  exit 1
fi
