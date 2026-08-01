#!/usr/bin/env bash
# Prune GitHub Actions caches that force coverage cold-compiles.
#
# - Delete every cache scoped to a PR ref (other PRs cannot restore them; they only
#   burn the 10GiB repo quota).
# - On main, for each v0-rust-* restore-key prefix, keep the newest cache and delete
#   older lockfile-hash suffixes.
#
# Requires: gh auth with actions:write (GITHUB_TOKEN in Actions is enough).
# Portable: no bash-4 associative arrays (macOS /bin/bash is 3.x).
set -euo pipefail

repo="${GITHUB_REPOSITORY:-}"
if [[ -z "${repo}" ]]; then
  repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
fi

tmp="$(mktemp)"
keepers="$(mktemp)"
trap 'rm -f "${tmp}" "${keepers}"' EXIT

page=1
: >"${tmp}"
while true; do
  chunk="$(
    gh api "repos/${repo}/actions/caches?per_page=100&page=${page}" \
      --jq '.actions_caches[] | [.id, .ref, .key, (.created_at // ""), (.last_accessed_at // "")] | @tsv'
  )"
  [[ -n "${chunk}" ]] || break
  printf '%s\n' "${chunk}" >>"${tmp}"
  count="$(printf '%s\n' "${chunk}" | wc -l | tr -d ' ')"
  [[ "${count}" -eq 100 ]] || break
  page=$((page + 1))
done

if [[ ! -s "${tmp}" ]]; then
  echo "No Actions caches to inspect."
  exit 0
fi

deleted=0

# 1) Drop all PR-scoped caches (PRs re-create what they need; they are invisible
#    to other PRs and inflate the 10GiB quota).
while IFS=$'\t' read -r id ref key _created _accessed; do
  [[ -n "${id}" ]] || continue
  case "${ref}" in
    refs/pull/*)
      echo "delete pr-scoped cache id=${id} ref=${ref} key=${key}"
      gh api --method DELETE "repos/${repo}/actions/caches/${id}" >/dev/null
      deleted=$((deleted + 1))
      ;;
  esac
done <"${tmp}"

# 2) On main, keep newest v0-rust-* cache per restore-key prefix; drop older hashes.
# Key shape from Swatinem/rust-cache: v0-rust-<shared>-<os>-<hash>-<lockhash>
# Sort by prefix then timestamp descending; keep first id per prefix.
: >"${keepers}"
awk -F '\t' '
  $2 == "refs/heads/main" && $3 ~ /^v0-rust-/ {
    key = $3
    sub(/-[^-]+$/, "", key) # restore-key prefix
    ts = ($5 != "" ? $5 : $4)
    print ts "\t" key "\t" $1 "\t" $3
  }
' "${tmp}" \
  | sort -r \
  | awk -F '\t' '
      !seen[$2]++ { print $3 }
    ' >"${keepers}"

while IFS=$'\t' read -r id ref key created accessed; do
  [[ -n "${id}" ]] || continue
  [[ "${ref}" == "refs/heads/main" ]] || continue
  [[ "${key}" == v0-rust-* ]] || continue
  if ! grep -qx "${id}" "${keepers}"; then
    echo "delete stale main rust cache id=${id} key=${key}"
    gh api --method DELETE "repos/${repo}/actions/caches/${id}" >/dev/null
    deleted=$((deleted + 1))
  fi
done <"${tmp}"

echo "Pruned ${deleted} Actions cache(s)."
