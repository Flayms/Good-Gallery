# M02 · Spec audit

**Status:** Done (approved 2026-10-26, self-approved: the user asked to work through the milestones without their input)
**Specs:** all (001–017), `docs/design.md`, `docs/glossary.md`

## Approach

The specs were copied unchanged during setup. This milestone does the parts that need judgment: requirements that only make sense for an Electron app, wording bound to web technology, and requirements the rewrite needs that the reference never had.

Rules applied:

- **Reword in place** when the behavior stays and only the mechanism named in the text was web-specific (e.g. "Back" as browser history → the mouse's back button). The REQ keeps its ID.
- **Deprecate** (strike through, reason, date) when the behavior itself goes away (the window as an untrusted web page, custom protocols, IPC). Deprecated REQs keep their text and their `no test (deprecated)` row.
- **Add** new REQs with the next free number of their spec.

Per spec:

| Spec | Change |
| ---- | ------ |
| 001 | + REQ-001-08 *Existing index*: an index written by the Electron app opens with its libraries, media and settings, and stays readable by it (schema and migration bookkeeping compatible). |
| 002 | REQ-002-13 reworded from "indexer process crash" to *Indexer failure*: a panic while scanning ends that scan, later scans still run, and the window stays responsive (the indexer is a thread, see `docs/architecture.md`). |
| 005 | + REQ-005-15 *Existing thumbnail cache*: thumbnails cached by the Electron app are shown without rendering them again. How the cache is laid out on disk is decided in M5; the REQ only asks that existing thumbnails keep working. |
| 006 | + REQ-006-12 *Frame-time budget* (p95 < 16 ms at 12 columns over 100k items, measured), + REQ-006-13 *Thumbnail memory budget* (GPU memory for thumbnails stays within a fixed budget; visible and upcoming tiles are kept). |
| 007 | Goal and REQ-007-13 no longer mention the URL: the search is app state that survives opening and closing the viewer. REQ-007-03's example ("from an old URL") replaced. |
| 009 | REQ-009-02 / -03: "Back" becomes the mouse's back button (and Alt + ←). REQ-009-12: no viewer URL; an item that vanishes while it is shown. Assumption about Chromium codecs replaced by the video backend. |
| 011 | REQ-011-01, -02, -03, -05, -06, -07 deprecated (no web page, renderer, IPC, navigation, permissions or content policy in a native app); REQ-011-04 deprecated (custom protocols) and replaced by REQ-011-08 *Files only from the index*. + REQ-011-09 *Helpers can't be steered by file names*, + REQ-011-10 *Hostile metadata*, + REQ-011-11 *No network access*. Goal, out of scope and assumptions rewritten. |
| 013 | Goal no longer mentions shadcn; assumptions about Chromium features replaced by the OS settings and the M0 glass measurement. |
| 014 | REQ-014-01 lists the Rust checks (format, clippy, spec check, tests incl. UI tests); REQ-014-07 sets the version in `Cargo.toml` instead of `package.json`. |
| 003, 004, 008, 010, 012, 015, 016, 017 | No change: their wording is technology-neutral. |

**Design guide.** `docs/design.md` keeps the look; its mechanics move to egui terms: tokens live in `crates/app/src/theme.rs` (Rust names in the tables instead of CSS utilities), glass is the wgpu blur pass from M0, the fallbacks follow the Windows settings *Transparency effects* and *Animation effects*, the focus ring and scrollbars are drawn by the app's own widgets. The *Decisions* table keeps its history and gets one row for the move to egui.

**Glossary.** *Search* is no longer "kept in the URL"; *Indexer* is "background threads", not "process".

**Plan.** The milestone scope doesn't change. M2's exit criterion "user approved the ported specs" becomes self-approval, as for the milestone plans.

## Reference

None beyond the specs themselves (`../good-gallery/docs/specs/` is identical to what was copied).

## Tasks

- [x] 1. 011: deprecations and new REQs, goal, out of scope, assumptions, Verification rows.
- [x] 2. 001, 002, 005, 006: new and reworded REQs, Verification rows.
- [x] 3. 007, 009, 013, 014: technology-bound wording.
- [x] 4. `docs/design.md` in egui terms; glossary.
- [x] 5. `cargo xtask check-specs` clean; specs README, progress.

## Exit criteria

From `docs/rewrite/plan.md`: specs approved (here: self-approved); `cargo xtask check-specs` runs clean (warnings for `no test` are expected).
