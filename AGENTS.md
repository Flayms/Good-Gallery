# AGENTS.md

Good Gallery, rewritten in Rust: a desktop gallery for large photo and video libraries (50k–500k files). Media appear in a virtualized masonry, justified, grid or list view. Libraries on network shares are supported, and thumbnails are cached locally. Search uses the tags that are already embedded in the files.

This repo is a **rewrite** of the Electron app in `../good-gallery` (the **reference**). The work is cut into milestones (`docs/rewrite/plan.md`); progress lives in `docs/progress.md`.

## Orientation

| Document                       | Why                                                                                     |
| ------------------------------ | --------------------------------------------------------------------------------------- |
| `docs/progress.md`             | **Read first.** Milestone status, current milestone, handoff log of previous sessions   |
| `docs/rewrite/plan.md`         | Master plan: target architecture, compatibility rules, milestones and their exit criteria |
| `docs/rewrite/milestones/`     | One plan per milestone (approach + tasks), written at the start of the milestone        |
| `docs/architecture.md`         | Target architecture and the decisions that outlive a milestone                          |
| `docs/glossary.md`             | Domain terms. Use them in code, tests and UI                                            |
| `docs/known-pitfalls.md`       | Add every pitfall you hit, so the next session avoids it                                |
| `docs/specs/`                  | Feature specs, ported from the reference. The contract for every milestone                |
| `docs/design.md`               | Visual design (adapted to egui in M2). Read before any UI work                   |

## The reference (`../good-gallery`)

- **Read-only.** Never edit, commit, branch or check out anything there. Running its commands (`pnpm test`, `pnpm build`, `pnpm dev`) is fine; they only write ignored output.
- **Order of authority:**
  1. **Behavior** comes from the specs in `docs/specs/`.
  2. **Details and edge cases** come from the reference code and its tests: SQL, exiftool tag names, layout math, orientation mapping, normalization rules. Port test cases, not just code.
  3. **Architecture** comes from `docs/rewrite/plan.md`, `docs/architecture.md` and the milestone plans here, **never** from the TypeScript structure. No tRPC-shaped modules, no React-shaped UI code, no Electron workarounds that Rust doesn't need.
- When reference code and spec disagree, the spec wins. Note the difference in the milestone plan and, if the spec is wrong, fix the spec.
- The user's real index (`%APPDATA%\good-gallery\gallery.db`) and thumbnail cache (`%APPDATA%\good-gallery\thumbs\`) are **read-only** too. Copy them into a temp folder before opening them.

## Session workflow: "continue with the next milestone"

One milestone (or one part of a split milestone) per session.

1. **Orient.** Read `docs/progress.md`. The current milestone is the first one that isn't `Done`. Read its section in `docs/rewrite/plan.md`, `docs/architecture.md`, `docs/known-pitfalls.md` and the specs it covers.
2. **Resume or plan.**
   - A milestone plan exists in `docs/rewrite/milestones/` and is approved: continue with its first unchecked task.
   - No plan yet: explore the reference files listed for the milestone, then write `docs/rewrite/milestones/MNN-name.md` (template below). Present it in plan mode and **wait for approval**. If the milestone is too large for one session (roughly more than 3k lines or 15 tasks), split it into parts (`M08a`, `M08b`) in the plan and in `progress.md`.
3. **Implement** task by task, TDD (red → green → refactor). Check off each task in the milestone plan as soon as it is done, so an interrupted session can be resumed.
4. **Verify.** All of these must pass in this session: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and (from M1 on) `cargo xtask check-specs`. UI changes also need a look at the running app or a snapshot (see M1).
5. **Record.** Update the Verification tables and spec statuses, record lasting decisions in `docs/architecture.md`, add pitfalls, then append a log entry to `docs/progress.md` (done, decisions, open issues, next step) and set the milestone status.
6. **Review.** Run the `code-review` skill on the changes and fix what holds up.
7. **Hand off.** Stop and summarize: milestone, REQs touched, results, open issues, and a conventional commit message. Don't start the next milestone.

Spikes in M0 live in `spikes/<name>/` as standalone crates outside the workspace. They are throwaway code: no specs, no TDD, but measured results and a go/no-go each, recorded in the M0 plan and summarized in `docs/architecture.md`.

## Project

- Stack (target, refined in `docs/architecture.md`): Rust, eframe / egui on wgpu, rusqlite (SQLite WAL), exiftool and ffmpeg as bundled helper binaries. Details in `docs/rewrite/plan.md`.
- Toolchain: pinned in `rust-toolchain.toml`. Windows needs the MSVC C++ build tools (see `docs/known-pitfalls.md`).
- Test: `cargo test --workspace` · Lint: `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo xtask check-specs` · Run: `cargo run -p app` (from M1 on).
- Pin exact crate versions for egui, eframe, egui_kittest and wgpu; their APIs change between minor versions. Check docs.rs for the pinned version instead of relying on memory.

## Principles

> [!NOTE]
> These are no absolutes, decide when it's appropriate to apply them.

- Follow SOLID principles where they help; decide when DRY, YAGNI and KISS apply.
- Spec-first, TDD (red → green → refactor), BDD acceptance criteria. DDD only where the domain warrants it; always use domain terms (`docs/glossary.md`).
- Keep pure logic (layout, search model, tag normalization, date groups) in crates without UI or IO dependencies, so it is easy to test.

## Tiers

0. No behavior change → tests pass.
1. Bug (code violates a REQ) → regression test referencing the REQ, then fix.
2. Small feature or spec gap → `spec.md` first. A wrong or missing spec is a spec change, not a bug.
3. Large or architectural → `spec.md` + plan (with tasks); wait for approval before implementing. Every milestone counts as Tier 3.

## Specs

`docs/specs/NNN-name/`, numbering kept from the reference. IDs are `REQ-NNN-XX`: never reused, deprecated rather than deleted, and referenced in tests. Spec and code change together.

- `spec.md`: Status, Goal, Requirements (Given/When/Then), Out of scope, Assumptions, Verification (REQ | test | result). No implementation details.
- Tests reference REQs at the start of their name: `fn req_007_01_filters_by_tag()` or a doc comment `/// REQ-007-01, REQ-007-02: …` directly above the test. The exact convention is settled in M1 and enforced by `cargo xtask check-specs`.

## Definition of Done (per milestone)

- Exit criteria of the milestone in `docs/rewrite/plan.md` are met.
- Every REQ in scope has a test that you saw fail, then pass.
- Full test suite and lint run in this session and pass.
- Verification tables, spec status and `docs/progress.md` updated.
- No unrelated changes.

## Git

Stay on the current branch. Commit or push only if explicitly asked for. Conventional commits (`feat:`, `fix:`, `docs:`, `chore:`, `test:`, `refactor:`).

## Milestone plan template

```markdown
# MNN · Title

**Status:** Draft | Approved | Done
**Specs:** NNN, NNN

## Approach

How, in a few paragraphs. Decisions (lasting ones also go into `docs/architecture.md`). Differences from the reference and why.

## Reference

Files in ../good-gallery read for this milestone and what was taken from them.

## Tasks

- [ ] 1. … (REQ-NNN-XX)
- [ ] 2. …

## Exit criteria

Copied from docs/rewrite/plan.md, plus anything added here.
```
