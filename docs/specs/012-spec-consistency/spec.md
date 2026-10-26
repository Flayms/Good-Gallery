# 012 · Spec consistency check

**Status:** Approved (ported from the reference, not yet implemented in Rust)

## Goal

Specs and tests reference each other: verification tables name the tests of a REQ, and tests mention the REQs they verify. A check run with the linters finds broken or stale references in either direction, so neither side silently drifts when tests are renamed, REQs are deprecated or gaps get closed.

## Requirements

### REQ-012-01 · Unknown REQs in tests

- **Given** a test file that mentions a REQ ID no spec defines
- **When** the check runs
- **Then** it reports an error naming the file and the ID

### REQ-012-02 · Deprecated REQs in tests

- **Given** a test file that mentions a deprecated REQ (heading struck through)
- **When** the check runs
- **Then** it reports an error naming the file and the ID

### REQ-012-03 · Complete verification tables

- **Given** a spec
- **When** the check runs
- **Then** it reports an error for every REQ without a row in the spec's verification table, every REQ with more than one row, and every row whose REQ isn't defined in that spec

### REQ-012-04 · Results match the tests

- **Given** a verification row of a REQ that isn't deprecated
- **When** the check runs
- **Then** it reports an error if the result is `pass` or `partial` but no test file mentions the REQ, if the result is `no test` but a test file mentions it, or if the result starts with anything other than `pass`, `partial` or `no test`

### REQ-012-05 · Valid REQ definitions

- **Given** the specs
- **When** the check runs
- **Then** it reports an error for a REQ defined more than once, and for a REQ whose number doesn't match its spec's folder number (`REQ-007-xx` belongs in `007-…`)

### REQ-012-06 · Gaps are warnings

- **Given** an `Implemented` spec with REQs whose result is `no test`
- **When** the check runs
- **Then** each is reported as a warning, which doesn't make the check fail

### REQ-012-07 · Exit status

- **Given** the check found errors
- **When** it finishes
- **Then** it exits with a non-zero status after listing all errors and warnings. Without errors it exits with status 0

### REQ-012-08 · Summary

- **Given** the `--summary` option
- **When** the check runs
- **Then** it also prints per spec how many REQs are `pass`, `partial` and `no test`

## Out of scope

- Judging whether a test actually proves its REQ (that's the `partial` judgement in the table).
- Running the tests or reading their results.
- Rewriting specs or test titles automatically.

## Assumptions

- A test file references a REQ by mentioning its ID anywhere in the file, by convention at the start of the test title (`it('REQ-007-01: …')`, e2e: `test.step('REQ-…: …')`).
- Test files are `*.test.ts(x)` below `src/` and `scripts/`, and every `.ts` file below `e2e/`.

## Verification

| REQ        | Test                                                                                   | Result |
| ---------- | -------------------------------------------------------------------------------------- | ------ |
| REQ-012-01 | — | no test |
| REQ-012-02 | — | no test |
| REQ-012-03 | — | no test |
| REQ-012-04 | — | no test |
| REQ-012-05 | — | no test |
| REQ-012-06 | — | no test |
| REQ-012-07 | — | no test |
| REQ-012-08 | — | no test |
