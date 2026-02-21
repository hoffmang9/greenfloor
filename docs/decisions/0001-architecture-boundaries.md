# 0001 - Architecture Boundaries

## Status
Accepted

## Decision

Use a layered architecture with deterministic core policies and side-effect adapters:

- `greenfloor/core` contains deterministic policy logic.
- `greenfloor/config` handles configuration parsing and validation.
- daemon/manager/notify modules orchestrate and perform side effects.

## Rationale

This supports maintainability, clear testing boundaries, and eventual Rust portability for domain logic.

## Consequences

- Core modules remain fast and easy to test with deterministic fixtures.
- IO-heavy integrations can be replaced without rewriting policy logic.
