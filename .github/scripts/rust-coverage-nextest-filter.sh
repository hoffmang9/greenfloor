#!/usr/bin/env bash
# Build nextest -E filter for incremental coverage from changed production paths on stdin.
# Always includes integration test binaries from greenfloor-engine/tests/*.rs; adds lib
# test tokens from every path segment under greenfloor-engine/src/.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
integration_tests_dir="${repo_root}/greenfloor-engine/tests"

add_unique() {
  local token="$1"
  local existing
  if [[ -z "${token}" ]] || [[ "${#token}" -lt 2 ]] || [[ "${token}" == "mod" ]]; then
    return
  fi
  for existing in "${tokens[@]:-}"; do
    if [[ "${existing}" == "${token}" ]]; then
      return
    fi
  done
  tokens+=("${token}")
}

tokens=()
expressions=()

test_file=""
for test_file in "${integration_tests_dir}"/*.rs; do
  [[ -f "${test_file}" ]] || continue
  expressions+=("binary(/$(basename "${test_file}" .rs)/)")
done

while IFS= read -r file; do
  [[ -n "${file}" ]] || continue
  [[ "${file}" =~ ^greenfloor-engine/src/(.+)\.rs$ ]] || continue
  local_path="${BASH_REMATCH[1]}"

  IFS='/' read -ra parts <<<"${local_path}"
  part=""
  for part in "${parts[@]}"; do
    add_unique "${part}"
  done
done

part=""
for part in "${tokens[@]:-}"; do
  expressions+=("test(/${part}/)")
done

expr=""
for part in "${expressions[@]}"; do
  if [[ -n "${expr}" ]]; then
    expr="${expr} | "
  fi
  expr="${expr}${part}"
done

printf '%s' "${expr}"
