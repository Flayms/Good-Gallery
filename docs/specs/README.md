# Specs

One folder per feature: `docs/specs/NNN-name/` with `spec.md` (what and why) and `plan.md` (how: approach, deviations, tasks; open tasks are unchecked). Workflow and tiers: see [AGENTS.md](../../AGENTS.md). Terms: see [glossary](../glossary.md). Open work across specs is tracked in [GitHub issues](https://github.com/Flayms/Good-Gallery/issues).

| Spec                                                     | Area                                                             | Status      |
| -------------------------------------------------------- | ---------------------------------------------------------------- | ----------- |
| [001-library-management](001-library-management/spec.md) | Add, remove and rescan libraries                                 | Implemented |
| [002-indexing](002-indexing/spec.md)                     | Scanning libraries into the index, scan queue, offline libraries | Implemented |
| [003-change-detection](003-change-detection/spec.md)     | Watcher, quick scans, scheduled checks                           | Implemented |
| [004-metadata](004-metadata/spec.md)                     | Dates, dimensions, ratings and tags from files and sidecars      | Implemented |
| [005-thumbnails](005-thumbnails/spec.md)                 | Thumbnail rendering, placeholders, thumbnail cache               | Implemented |
| [006-gallery-views](006-gallery-views/spec.md)           | Masonry, Justified, Grid and List views, zoom                    | Implemented |
| [007-search](007-search/spec.md)                         | Tag, file name, date, rating, kind and folder filters, sorting   | Implemented |
| [008-sidebar](008-sidebar/spec.md)                       | Libraries, folders, People, Places, Ratings, Tags, group order   | Implemented |
| [009-media-viewer](009-media-viewer/spec.md)             | Viewer, zoom and pan, info panel, context menu                   | Implemented |
| [010-settings](010-settings/spec.md)                     | Indexing and thumbnail cache settings                            | Implemented |
| [011-security](011-security/spec.md)                     | File access, helper programs, hostile metadata, no network       | Implemented |
| [012-spec-consistency](012-spec-consistency/spec.md)     | Check that specs and tests reference each other                  | Implemented |
| [013-visual-design](013-visual-design/spec.md)           | Glass chrome, typeface, compact toolbar, viewer over the blur    | Implemented |
| [014-ci-release](014-ci-release/spec.md)                 | CI checks, release script, changelog and GitHub releases         | Implemented |
| [015-filmstrip](015-filmstrip/spec.md)                   | Row of thumbnails in the viewer to jump through the gallery      | Implemented |
| [016-date-groups](016-date-groups/spec.md)               | Group the gallery by day, with date and count headers            | Implemented |
| [017-tag-indicators](017-tag-indicators/spec.md)         | Rating, Places, People and Tags icons over tiles                 | Implemented |
| [018-linux](018-linux/spec.md)                           | Linux: data folder, system helpers, title bar, package           | Implemented |
| [019-web](019-web/spec.md)                               | Browser version: a static page, index kept in the browser        | Approved    |
| [020-light-mode](020-light-mode/spec.md)                 | Light appearance, following the system or chosen in the settings | Implemented |
| [021-photo-editing](021-photo-editing/spec.md)           | Light, curves, color, effects in the viewer; reversible saves    | Implemented |
| [022-subject-and-background](022-subject-and-background/spec.md) | Light and color of a photo's subject and background, found automatically | Implemented |

## Conventions

- **Status:** `Draft` → `Approved` (plan accepted, Tier 3) → `Implemented`. `Deprecated` for removed features.
- **REQ IDs:** `REQ-NNN-XX`. Never reused or renumbered. Deprecate with `~~strikethrough~~` and a reason instead of deleting.
- **Verification results:** `pass` (a test covers the REQ and passes), `partial` (tests cover only the named part), `no test` (only manual or by inspection). Close `partial` and `no test` gaps when the REQ is next touched.
- **Tests reference REQs** at the start of their name (`fn req_007_01_filters_by_tag()`, several chained) or in a doc comment directly above them (`/// REQ-007-01, REQ-007-02: …`); only the test part of a file counts (from the first `#[cfg(test)]` on, or files below `tests/` and `benches/`). `cargo xtask check-specs` fails on broken or stale references in either direction and warns about `no test` REQs.

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
