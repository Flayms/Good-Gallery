# Specs

One folder per feature: `docs/specs/NNN-name/` with `spec.md` (what and why). Large or architectural work gets a plan in `docs/rewrite/milestones/` during the rewrite, and a `plan.md` in the spec folder afterwards. Workflow and tiers: see [AGENTS.md](../../AGENTS.md). Terms: see [glossary](../glossary.md).

All specs were ported from the reference (`../good-gallery/docs/specs/`) on 2026-10-26 with their requirements unchanged. Their Verification tables were reset, since the reference tests don't run here. The reference's per-spec `plan.md` files describe the TypeScript approach and were not copied; read them there for background. M2 (2026-10-26) deprecated the Electron-specific REQs (most of 011), reworded technology-bound ones and added the REQs the rewrite needs (data compatibility, frame-time and memory budgets, helper and metadata safety); see `docs/rewrite/milestones/M02-spec-audit.md`.

| Spec                                                     | Area                                                             | Status   | Milestone |
| -------------------------------------------------------- | ---------------------------------------------------------------- | -------- | --------- |
| [001-library-management](001-library-management/spec.md) | Add, remove and rescan libraries                                 | Implemented | M3        |
| [002-indexing](002-indexing/spec.md)                     | Scanning libraries into the index, scan queue, offline libraries | Approved | M4        |
| [003-change-detection](003-change-detection/spec.md)     | Watcher, quick scans, scheduled checks                           | Implemented | M4        |
| [004-metadata](004-metadata/spec.md)                     | Dates, dimensions, ratings and tags from files and sidecars      | Implemented | M4        |
| [005-thumbnails](005-thumbnails/spec.md)                 | Thumbnail rendering, placeholders, thumbnail cache               | Implemented | M5        |
| [006-gallery-views](006-gallery-views/spec.md)           | Masonry, Justified, Grid and List views, zoom                    | Implemented | M6        |
| [007-search](007-search/spec.md)                         | Tag, file name, date, rating, kind and folder filters, sorting   | Implemented | M7        |
| [008-sidebar](008-sidebar/spec.md)                       | Libraries, folder tree, People, Places, Ratings, Tags            | Implemented | M7        |
| [009-media-viewer](009-media-viewer/spec.md)             | Viewer, zoom and pan, info panel, context menu                   | Implemented | M8, M9    |
| [010-settings](010-settings/spec.md)                     | Indexing and thumbnail cache settings                            | Implemented | M10       |
| [011-security](011-security/spec.md)                     | File access, helper programs, hostile metadata, no network       | Implemented | M4, M5, M8 |
| [012-spec-consistency](012-spec-consistency/spec.md)     | Check that specs and tests reference each other                  | Implemented | M1        |
| [013-visual-design](013-visual-design/spec.md)           | Glass chrome, typeface, compact toolbar, viewer over the blur    | Implemented | M10       |
| [014-ci-release](014-ci-release/spec.md)                 | CI checks, release script, changelog and GitHub releases         | Implemented | M1, M11   |
| [015-filmstrip](015-filmstrip/spec.md)                   | Row of thumbnails in the viewer to jump through the gallery      | Implemented | M8        |
| [016-date-groups](016-date-groups/spec.md)               | Group the gallery by day, with date and count headers            | Implemented | M6        |
| [017-tag-indicators](017-tag-indicators/spec.md)         | Rating, Places, People and Tags icons over tiles                 | Implemented | M6        |

## Conventions

- **Status:** `Draft` → `Approved` (accepted, not implemented here yet) → `Implemented`. `Deprecated` for removed features.
- **REQ IDs:** `REQ-NNN-XX`. Never reused or renumbered. Deprecate with `~~strikethrough~~` and a reason instead of deleting.
- **Verification results:** `pass` (a test covers the REQ and passes), `partial` (tests cover only the named part), `no test` (only manual or by inspection). Close `partial` and `no test` gaps when the REQ is next touched.
- **Tests reference REQs** at the start of their name or in a doc comment directly above them; the exact Rust convention is settled in M1. `cargo xtask check-specs` fails on broken or stale references in either direction and warns about `no test` REQs.

## Template

```markdown
# NNN · Title

**Status:** Draft

## Goal

One paragraph: who benefits and why.

## Requirements

### REQ-NNN-01 · Short name

- **Given** …
- **When** …
- **Then** …

## Out of scope

## Assumptions

## Verification

| REQ        | Test                         | Result |
| ---------- | ---------------------------- | ------ |
| REQ-NNN-01 | `crates/x/src/y.rs` › test   | pass   |
```
