#!/usr/bin/env bash
# Unit tests for incremental coverage path scope and nextest filter helpers.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../../.." && pwd)"

fixture_dirs=()

cleanup() {
  local dir
  for dir in "${fixture_dirs[@]:-}"; do
    rm -rf "${dir}"
  done
}
trap cleanup EXIT

assert_contains() {
  local haystack="$1"
  local needle="$2"
  local message="$3"
  if [[ "${haystack}" != *"${needle}"* ]]; then
    echo "ASSERT FAILED: ${message}" >&2
    echo "  expected substring: ${needle}" >&2
    echo "  actual: ${haystack}" >&2
    exit 1
  fi
}

assert_not_contains() {
  local haystack="$1"
  local needle="$2"
  local message="$3"
  if [[ "${haystack}" == *"${needle}"* ]]; then
    echo "ASSERT FAILED: ${message}" >&2
    echo "  unexpected substring: ${needle}" >&2
    echo "  actual: ${haystack}" >&2
    exit 1
  fi
}

assert_eq() {
  local actual="$1"
  local expected="$2"
  local message="$3"
  if [[ "${actual}" != "${expected}" ]]; then
    echo "ASSERT FAILED: ${message}" >&2
    echo "  expected: ${expected}" >&2
    echo "  actual: ${actual}" >&2
    exit 1
  fi
}

new_fixture_dir() {
  local fixture_dir
  fixture_dir="$(mktemp -d)"
  fixture_dirs+=("${fixture_dir}")
  printf '%s' "${fixture_dir}"
}

init_git_repo() {
  git init --quiet
  git config user.email "test@example.com"
  git config user.name "test"
}

echo "changed-production-rust-files excludes tests.rs and test_support"
fixture_dir="$(new_fixture_dir)"
(
  cd "${fixture_dir}"
  init_git_repo
  mkdir -p greenfloor-engine/src/offer/bootstrap
  touch greenfloor-engine/src/offer/bootstrap/replan.rs
  git add .
  git commit --quiet -m "base"
  mkdir -p \
    greenfloor-engine/src/offer/operator/signer_denomination/bootstrap_execute \
    greenfloor-engine/src/test_support
  echo "// delta" >> greenfloor-engine/src/offer/bootstrap/replan.rs
  touch \
    greenfloor-engine/src/offer/operator/signer_denomination/bootstrap_execute/tests.rs \
    greenfloor-engine/src/test_support/eco181_bootstrap_inventory.rs
  git add .
  git commit --quiet -m "delta"
  changed="$(
    bash "${repo_root}/.github/scripts/changed-production-rust-files.sh" HEAD~1
  )"
  assert_contains "${changed}" "greenfloor-engine/src/offer/bootstrap/replan.rs" "replan counted"
  assert_not_contains "${changed}" "tests.rs" "tests.rs excluded"
  assert_not_contains "${changed}" "test_support" "test_support excluded"
)

echo "changed-production-rust-files accepts stdin path list"
changed="$(
  printf '%s\n' \
    'greenfloor-engine/src/offer/bootstrap/replan.rs' \
    'greenfloor-engine/src/test_support/eco181.rs' \
    | bash "${repo_root}/.github/scripts/changed-production-rust-files.sh" -
)"
assert_contains "${changed}" "replan.rs" "stdin replan counted"
assert_not_contains "${changed}" "test_support" "stdin test_support excluded"

echo "tests-only changes do not enable rust diff coverage"
fixture_dir="$(new_fixture_dir)"
(
  cd "${fixture_dir}"
  init_git_repo
  mkdir -p greenfloor-engine/src/offer/bootstrap
  touch greenfloor-engine/src/offer/bootstrap/replan.rs
  git add .
  git commit --quiet -m "base"
  mkdir -p \
    greenfloor-engine/src/offer/operator/signer_denomination/bootstrap_execute \
    greenfloor-engine/src/test_support
  touch \
    greenfloor-engine/src/offer/operator/signer_denomination/bootstrap_execute/tests.rs \
    greenfloor-engine/src/test_support/eco181_bootstrap_inventory.rs
  git add .
  git commit --quiet -m "tests only"
  export GITHUB_OUTPUT="$(mktemp)"
  bash "${repo_root}/.github/scripts/diff-coverage-scope.sh" HEAD~1 >/dev/null
  # shellcheck disable=SC1090
  source "${GITHUB_OUTPUT}"
  assert_eq "${run_rust_cov}" "false" "tests-only diff should not run rust cov"
)

echo "scope and changed-files agree when production Rust changes"
fixture_dir="$(new_fixture_dir)"
(
  cd "${fixture_dir}"
  init_git_repo
  mkdir -p greenfloor-engine/src/offer/bootstrap
  touch greenfloor-engine/src/offer/bootstrap/replan.rs
  git add .
  git commit --quiet -m "base"
  echo "// delta" >> greenfloor-engine/src/offer/bootstrap/replan.rs
  git add .
  git commit --quiet -m "delta"
  export GITHUB_OUTPUT="$(mktemp)"
  bash "${repo_root}/.github/scripts/diff-coverage-scope.sh" HEAD~1 >/dev/null
  # shellcheck disable=SC1090
  source "${GITHUB_OUTPUT}"
  assert_eq "${run_rust_cov}" "true" "production rust diff should run rust cov"
)

echo "nextest filter includes integration binaries and changed path tokens"
filter="$(
  printf '%s\n' 'greenfloor-engine/src/config/program.rs' \
    | bash "${repo_root}/.github/scripts/rust-coverage-nextest-filter.sh"
)"
assert_contains "${filter}" "binary(/config/)" "integration config binary"
assert_contains "${filter}" "test(/program/)" "program path token"
assert_contains "${filter}" "test(/config/)" "config path token"

filter="$(
  printf '%s\n' 'greenfloor-engine/src/offer/bootstrap/replan.rs' \
    | bash "${repo_root}/.github/scripts/rust-coverage-nextest-filter.sh"
)"
assert_contains "${filter}" "binary(/daemon_once_integration/)" "integration binaries from tests/*.rs"
assert_contains "${filter}" "test(/replan/)" "replan path token"
assert_contains "${filter}" "test(/bootstrap/)" "bootstrap path token"
assert_contains "${filter}" "test(/offer/)" "offer path token"

filter="$(
  {
    printf '%s\n' 'greenfloor-engine/src/offer/bootstrap/replan.rs'
    printf '%s\n' 'greenfloor-engine/src/test_support/eco181_shape_cases.rs'
  } | bash "${repo_root}/.github/scripts/rust-coverage-nextest-filter.sh"
)"
assert_contains "${filter}" "test(/eco181_shape_cases/)" "changed test_support path token"

filter="$(
  printf '%s\n' 'greenfloor-engine/src/storage/sqlite/schema.rs' \
    | bash "${repo_root}/.github/scripts/rust-coverage-nextest-filter.sh"
)"
assert_contains "${filter}" "test(/storage/)" "storage ancestor path token"
assert_contains "${filter}" "test(/sqlite/)" "sqlite path token"
assert_contains "${filter}" "binary(/sqlite_store/)" "discovered sqlite integration binary"

echo "all rust coverage script tests passed"
