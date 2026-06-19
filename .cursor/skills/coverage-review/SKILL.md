---
name: coverage-review
description: >-
  Analyse test coverage gaps and report uncovered code before making changes.
  Use when the user invokes /coverage-review, asks for a coverage assessment,
  coverage report, test coverage gaps, uncovered code, or low-coverage files.
argument-hint: "[files] [instructions]"
user-invocable: true
context: fork
disable-model-invocation: true
---

# Coverage Review

Analyse the project's test coverage, identify gaps, and present an assessment for the user to review before any changes are made. Do not write or modify any code until the user provides next steps.

Files and instructions: $ARGUMENTS

## What to detect

Before running anything, determine the test framework, runner, and coverage tooling by inspecting the project:

- **JavaScript/TypeScript**: jest --coverage, vitest --coverage, c8, nyc/istanbul — check package.json scripts, config files, and dev dependencies for coverage providers (@vitest/coverage-v8, @vitest/coverage-istanbul, c8, nyc)
- **Python**: pytest --cov, coverage.py — check pyproject.toml, setup.cfg, .coveragerc
- **Go**: go test -cover, go test -coverprofile — built-in
- **Rust**: cargo tarpaulin, cargo llvm-cov — check Cargo.toml dev-dependencies. If the project uses **cargo nextest** (check CI, Makefile, or docs), run coverage through nextest (for example `cargo llvm-cov nextest run`) rather than plain `cargo test`
- **Java/Kotlin**: jacoco, cobertura — check pom.xml or build.gradle plugins
- **Ruby**: simplecov — check Gemfile
- **Elixir**: mix test --cover, excoveralls — check mix.exs
- **PHP**: phpunit --coverage-text — check phpunit.xml, composer.json

If no coverage tool is installed, recommend one appropriate for the detected framework and ask the user before installing it.

## Multi-language repos

Many repos contain more than one test stack. When multiple frameworks are present:

1. Detect each stack independently (for example Rust + Python, or app + scripts).
2. Run a separate coverage report per stack using that stack's runner and config.
3. Apply each stack's own excludes (coverage config, `extend-exclude`, submodule paths).
4. Merge findings into one assessment with a **per-stack** framework line and a combined summary table.
5. Do not stop after the first detected framework — incomplete polyglot analysis is a common failure mode.

## Coverage thresholds

Use the project's configured threshold when one exists (for example in coverage config or CI). Otherwise apply these defaults consistently:

| Tier | Line coverage | Treatment |
|------|---------------|-----------|
| **Uncovered** | Not in report / 0% | List in "Uncovered files"; highest priority when risk is high |
| **Low** | Below 50% | List in "Low-coverage files"; prioritize in recommendations |
| **Moderate** | 50–79% | Mention in summary counts; include in recommendations only when risk is high |
| **Acceptable** | 80% and above | Omit from gap tables unless a specific function or branch is uncovered |

A project is **COMPLETE** only when all in-scope source files are at or above the acceptable threshold (default 80%).

## How to interpret arguments

The arguments are free-form and flexible. They may contain:

- File or directory references to scope the analysis: `src/services/`, `@auth.ts`, `lib/`, `*.py`
- Natural language instructions such as:
  - "focus on the API layer"
  - "only check the utils module"
  - "ignore generated files"
  - "include integration tests"

When no arguments are provided, analyse coverage for the entire project.

### `@file` resolution

A path prefixed with `@` (for example `@auth.ts`) refers to a single file:

1. **Primary**: the file open or focused in the editor when the user invoked the skill.
2. **Fallback**: if no editor context is available, search the workspace for a unique basename match.
3. **Ambiguous**: if multiple paths share the basename, list the matches and ask the user to pick one before continuing.

### Examples

- `/coverage-review` — full project coverage analysis
- `/coverage-review src/services/` — coverage for a specific directory
- `/coverage-review @auth.ts` — coverage for a specific file
- `/coverage-review focus on the API handlers` — scoped by description
- `/coverage-review ignore generated files and vendor/` — with exclusions

## How to proceed

1. **Detect the framework and coverage tool**: read package.json, pyproject.toml, Cargo.toml, go.mod, or equivalent to identify available coverage tooling and its configuration. For multi-language repos, repeat per stack (see **Multi-language repos**).
2. **Determine scope**: if the user specified files or directories, limit the analysis to those. Otherwise analyse the full project.
3. **Run the coverage report**: execute the appropriate coverage command with text/summary output. Use flags that produce per-file and per-function breakdowns where available. Set a reasonable timeout (default 120s; on timeout, double once and retry; cap at 600s, then report partial results).
4. **Build source inventory and find truly uncovered files**:
   - Enumerate in-scope source files for each stack (respect user scope and natural-language exclusions).
   - Apply project excludes from coverage config, `.gitignore`-class paths, generated/vendor/submodule dirs, and any user-requested exclusions.
   - Subtract files that appear in the coverage report (including files reported at 0% — those were loaded but not exercised).
   - Files in the inventory but absent from the report are **uncovered** (never executed by any test).
5. **Parse the results**: extract file-level and function-level coverage data. Identify:
   - Uncovered files — from step 4 (inventory minus report), not from the report alone
   - Low-coverage files — in-scope files below 50% line coverage
   - Moderate-coverage files — in-scope files at 50–79% (summarize count; detail only high-risk items)
   - Uncovered functions/methods — specific functions with 0% coverage
   - Uncovered branches — conditional paths that are never exercised
6. **Read the uncovered code**: for each significant gap, read the source file to understand what the uncovered code does. Categorise each gap by risk and complexity:
   - **Risk**: how likely is a bug in this code to cause user-facing impact? (high / medium / low)
   - **Complexity**: how complex would the tests be to write? (simple / moderate / complex)
7. **Present the assessment**: report findings using the output format below. Stop here and wait for the user's instructions before writing any code.

## Output format

### Coverage assessment

```markdown
## Coverage Assessment

**Framework**: vitest + @vitest/coverage-v8 (or per-stack lines for multi-language repos)
**Scope**: full project (or scoped description)
**Overall line coverage**: 64% (target: 80%)

---

### Summary

| Category              | Count |
|-----------------------|-------|
| Uncovered files       | 4     |
| Low-coverage files    | 6     |
| Moderate-coverage     | 3     |
| Uncovered functions   | 12    |

---

### Uncovered files (no tests)

| File                          | Lines | Risk   | Complexity | What it does                          |
|-------------------------------|-------|--------|------------|---------------------------------------|
| `src/services/billing.ts`     | 142   | high   | moderate   | Stripe billing lifecycle management   |
| `src/utils/retry.ts`          | 38    | medium | simple     | Generic retry with exponential backoff|

### Low-coverage files (below 50%)

| File                          | Coverage | Uncovered functions              | Risk   | Complexity |
|-------------------------------|----------|----------------------------------|--------|------------|
| `src/handlers/auth.ts`        | 32%      | `refreshToken`, `revokeSession`  | high   | moderate   |
| `src/repos/order.ts`          | 45%      | `bulkUpdate`, `archiveOld`       | medium | complex    |

### Key uncovered branches

| Location                          | Branch description                       | Risk   |
|-----------------------------------|------------------------------------------|--------|
| `src/services/user.ts:52-58`      | Error path when email already exists     | high   |
| `src/handlers/auth.ts:91-95`      | Token expiry edge case                   | medium |

---

### Recommended priority

1. **`src/services/billing.ts`** — high risk, no tests at all, moderate complexity
2. **`src/handlers/auth.ts` → `refreshToken`, `revokeSession`** — high risk, auth-critical paths
3. **`src/services/user.ts:52-58`** — high risk branch, simple to cover
4. ...

---

What would you like to cover? You can point at specific files, pick from the priorities above, or ask me to cover everything.
```

### Cannot determine coverage tool

```markdown
## Coverage Assessment: UNKNOWN

Could not detect a coverage tool. Looked for: jest/vitest coverage config, pytest-cov, coverage.py, go test -cover, cargo tarpaulin, cargo llvm-cov, jacoco, simplecov.

Recommended tool for this project: **[recommendation based on detected test framework]**

Would you like me to install and configure it?
```

### No gaps found

```markdown
## Coverage Assessment: COMPLETE

**Overall line coverage**: 94% (target: 80%)

All in-scope files are at or above the acceptable threshold. No significant uncovered functions or branches detected.

Minor gaps (cosmetic):
- `src/config/defaults.ts:12` — unreachable fallback branch
- `src/index.ts:3-5` — top-level bootstrap (not practically testable)
```

## Important notes

- Do not write tests or modify code — this skill produces an assessment only. Wait for the user to decide what to cover and how
- Never run tests in watch mode — it requires interactive input
- Timeout handling — if a coverage collection exceeds the timeout, report what is completed and that the run was interrupted
- Respect existing configuration — use the project's existing coverage config (thresholds, exclusions, reporters) rather than overriding with custom flags
- Large projects — if the project has hundreds of source files, focus the detailed analysis on the user-specified scope or the top 15 lowest-coverage files to keep the report actionable
