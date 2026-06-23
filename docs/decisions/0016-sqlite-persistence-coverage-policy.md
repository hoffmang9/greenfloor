# ADR 0016: SQLite persistence coverage policy

## Status

Accepted (2026-06-23)

## Context

`storage/sqlite/` persistence code produces large llvm-cov function counts on defensive
`map_err` arms and migration glue. Coverage-driven unit tests for those paths added bulk
without strengthening product contracts. Integration suites in `greenfloor-engine/tests/sqlite_*`
already exercise persistence behavior end-to-end.

## Decision

1. **Exclude `storage/sqlite/` from llvm-cov reports and diff-cover gates.** Configuration lives
   in `.llvm-cov.toml` (`ignore-filename-regex`) and `.github/workflows/ci.yml`
   (`diff-cover --exclude '**/storage/sqlite/**'`). Do not duplicate with
   `#[coverage(off)]` attributes in source.

2. **Exclude `storage/test_support.rs` the same way.** It exists only for integration tests
   that seed pre-migration DB state before `SqliteStore::open` runs migrations.

3. **Do not add padding tests** whose sole purpose is hitting SQLite error-mapping arms or
   trivial parameter variants (for example optional filter `None` vs `Some`). Add integration
   tests in `tests/sqlite/` when behavior or contracts change.

4. **Changed persistence code is validated by integration tests**, not diff-cover on
   `storage/sqlite/` lines. Operator policy and offer/daemon paths remain in coverage scope.

## Consequences

- CI diff-cover no longer fails on untested `map_err` closures in sqlite modules.
- Regressions in persistence require `cargo test --test sqlite_store`,
  `cargo test --test sqlite_migrations`, and related suites to catch.
- New sqlite behavior should get an integration test in the appropriate `tests/sqlite/` module.
