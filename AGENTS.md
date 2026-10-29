# AGENTS.md

A desktop gallery for large photo and video libraries (50k–500k files). Media appear in a virtualized masonry, justified, grid or list view. Libraries on network shares are supported, and thumbnails are cached locally. Search uses the tags that are already embedded in your files.

## Orientation

| Document                 | Why                                                                                                               |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| `README.md`              | User facing description. Keep up to date with screenshots (`cargo xtask screenshots`).                            |
| `docs/architecture.md`   | Architecture overview and the decisions behind it                                                                 |
| `docs/design.md`         | Visual design: principles, tokens, surfaces, type, motion, writing. Read before any UI work                       |
| `docs/glossary.md`       | Definitions of domain terms                                                                                       |
| `docs/known-pitfalls.md` | if you encounter problems or pitfalls add them here, either to avoid them in the future or for us to fix later on |
| `docs/specs/README.md`   | Index of feature specs with their status, conventions and template                                                |

## Project

- Stack: Rust, eframe / egui on wgpu, rusqlite (SQLite WAL), exiftool, ffmpeg and libmpv as bundled helpers. Details in `docs/architecture.md`.
- Toolchain: pinned in `rust-toolchain.toml`. Windows needs the MSVC C++ build tools (see `docs/known-pitfalls.md`).
- Test: `cargo test --workspace` · Lint: `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo xtask check-specs` · Run: `cargo run -p app` · Release: `cargo xtask release` (conventional commits, see `README.md`)
- UI changes need a look at the result: a snapshot test (`UPDATE_SNAPSHOTS=1` rewrites them) or `cargo run -p app -- --screenshot <file.png>`.
- Pin exact crate versions for egui, eframe, egui_kittest and wgpu; their APIs change between minor versions. Check docs.rs for the pinned version instead of relying on memory.
- The user's real index and thumbnail cache (`%APPDATA%\good-gallery`) are read-only for development. Copy them into a temp folder before opening them. Development builds use `%APPDATA%\good-gallery-rust`.

## Principles

> [!NOTE]
> These are no absolutes, decide when it's appropriate to apply them.

- Follow SOLID principles where they help; decide when DRY, YAGNI and KISS apply.
- Spec-first, TDD (red → green → refactor), BDD acceptance criteria. DDD only where the domain warrants it; always use domain terms (`docs/glossary.md`).
- Keep pure logic (layout, search model, tag normalization, date groups) in `domain`, without UI or IO dependencies, so it is easy to test.

## Tiers

0. No behavior change → tests pass.
1. Bug (code violates a REQ) → regression test referencing the REQ, then fix.
2. Small feature or spec gap → `spec.md` first. A wrong or missing spec is a spec change, not a bug.
3. Large or architectural → `spec.md` + `plan.md` (with tasks); wait for approval before implementing.

## Specs

`docs/specs/NNN-name/`. IDs are `REQ-NNN-XX`: never reused, deprecated rather than deleted, and referenced in tests. Spec and code change together.

- `spec.md`: Status, Goal, Requirements (Given/When/Then), Out of scope, Assumptions, Verification (REQ | test | result). No implementation details.
- `plan.md`: Approach, Tasks (linked to REQs). Lasting decisions also go into `docs/architecture.md`.
- Tests reference REQs at the start of their name (`fn req_007_01_filters_by_tag()`) or in a doc comment directly above them (`/// REQ-007-01, REQ-007-02: …`). `cargo xtask check-specs` enforces it.

## Issues

- If the user references an issue, look it up at [GitHub issues](https://github.com/Flayms/Good-Gallery/issues).  
- Name branches related to issues `{issue-id}-{issue-name}`
- in your PR use a Github closing keyword for auto closing after merge

## Definition of Done

- Every REQ in scope has a test that you saw fail, then pass.
- Full test suite and lint run in this session and pass.
- Verification table and spec status updated.
- PR to main is green and mergeable.
- Summary: tier, REQs touched, results, open issues.

## Git

Create a new worktree for a new feature, branched from `origin/main`. Use conventional commits. When you are done, merge `origin/main` into the branch and create a PR to main. Then wait for CI and fix whatever turns red (on conflicts, merge `origin/main` again) until it is green and mergeable. Merged worktrees are removed at session start (`.claude/hooks/tidy-worktrees.sh`).

## Note

For issues and pull requests you create put the following note at the top of the description:

```md
> [!Note]

🤖 {Model e.g. Claude Opus 5.5} writing on behalf of {Human}
```
