# 012 · Spec consistency · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **`cargo xtask check-specs`** (`xtask/src/check_specs.rs`): parses every `docs/specs/*/spec.md` (headings `### REQ-…` and `### ~~REQ-…`, `**Status:**`, Verification rows) and the test part of every `.rs` file below `crates/` and `xtask/`: everything from the first `#[cfg(test)]` on, or the whole file below a `tests/` or `benches/` folder. REQs mentioned in production code don't count as coverage.
- **References:** at the start of a test name (`fn req_012_01_…`, several chained: `fn req_002_01_req_002_02_…`) or in a doc comment directly above the test (`/// REQ-012-01: …`). Both spellings count.
- Pure `check_specs(specs, tests) -> Report` plus `run(root, args, log) -> i32`, so both are unit-tested without a process. The check's own tests build REQ IDs at runtime, so they aren't counted as references. `--summary` prints counts per spec.

## Tasks

- [x] Parser and checks (REQ-012-01 … -06, -08)
- [x] `run` over the repository with the test-part rule, exit status, `--summary` (REQ-012-07, -08)
