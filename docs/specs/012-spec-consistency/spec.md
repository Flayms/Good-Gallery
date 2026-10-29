# 012 · Spec consistency check

**Status:** Implemented

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

- A test references a REQ by mentioning its ID in the test part of a file, by convention at the start of the test name (`fn req_007_01_filters_by_tag()`, the ID in lower case with underscores; several chained at the start: `fn req_007_01_req_007_02_…()`) or in a doc comment directly above the test (`/// REQ-007-01, REQ-007-02: …`). Both spellings count.
- Test files are the `.rs` files below `crates/` and `xtask/` (except `target/` folders), and the browser tests `web/tests/*.spec.js` (spec 019). Rust keeps unit tests next to the code, so only the test part of a file counts: everything from the first `#[cfg(test)]` on, or the whole file below a `tests/` or `benches/` folder. A REQ mentioned in production code is not a reference.

## Verification

| REQ        | Test                                                                                   | Result |
| ---------- | -------------------------------------------------------------------------------------- | ------ |
| REQ-012-01 | `xtask/src/check_specs.rs` › reports_test_references_to_unknown_reqs, counts_test_names_as_references | pass |
| REQ-012-02 | `xtask/src/check_specs.rs` › reports_test_references_to_deprecated_reqs | pass |
| REQ-012-03 | `xtask/src/check_specs.rs` › reports_missing_duplicate_and_unknown_verification_rows | pass |
| REQ-012-04 | `xtask/src/check_specs.rs` › reports_results_that_dont_match_the_tests | pass |
| REQ-012-05 | `xtask/src/check_specs.rs` › reports_duplicate_reqs_and_reqs_in_the_wrong_spec | pass |
| REQ-012-06 | `xtask/src/check_specs.rs` › reports_untested_reqs_of_implemented_specs_as_warnings | pass |
| REQ-012-07 | `xtask/src/check_specs.rs` › exits_non_zero_only_on_errors | pass |
| REQ-012-08 | `xtask/src/check_specs.rs` › summarizes_results_per_spec, prints_the_summary_on_request | pass |
