# 026 · Tagging · Plan

## Approach

### Decisions

Taken without asking, as the user said to go ahead:

1. **Same targets as ratings (spec 024).** An added tag goes into a photo, and into a video's sidecar unless the video has keywords itself. A removed tag leaves the file and its sidecar, because indexing merges the tags of both (REQ-004-10).
2. **Read, change, write back whole lists.** `media::tagging` reads the keyword fields with exiftool (`XMP-lr:HierarchicalSubject`, `XMP-dc:Subject`, `IPTC:Keywords`, `XMP-pdf:Keywords`, `IFD0:XPKeywords`, `XMP-iptcExt:PersonInImage`), `domain::tagging::{add, remove}` compute the new lists, and only changed fields are written (`-Tag=` empties a list, the values after it refill it). Pure list logic stays testable without exiftool. `XMP-pdf:Keywords` is there because exiftool turns IPTC keywords into it when it makes a sidecar from a file, and indexing reads it as `Keywords`.
3. **Removing a nested tag also removes its hierarchy's levels from the flat lists**, unless another hierarchy of the item still uses them. Lightroom writes every level as a flat keyword, and without this, `Places` and `Italy` would show up as tags of their own once `Places|Italy|Rome` is gone.
4. **People from face regions are removed with their regions** (spec 032, replacing a rule that left other programs' regions alone): removing the name from the keywords alone wouldn't remove the tag, since indexing reads region names too.
5. **The index reads the file again** (`db::media::mark_for_reading`, then a changes scan of the path), instead of the app writing tags into the index itself. Indexing's rules (REQ-004-07, REQ-004-08, REQ-004-11) then apply as for any file, and the info panel, the sidebar and the search follow when the scan has run, a moment later.
6. **Suggestions come from the sidebar's data**, already loaded off the UI thread: the People and Places trees as paths, and up to 5,000 other tags. Matching names (containing what was typed, starting ones first) show under the field. Enter takes an existing tag of that name with its hierarchy, else makes a new one under People or Places, or a flat tag.
7. **While a text field has the keyboard, the viewer's shortcuts are off.** Typing `i`, `f`, `e` or `p` would toggle its panels, and Escape would close it. egui takes the focus away before a frame with Escape starts, so the viewer also remembers whether a field had the keyboard at the end of the last frame.

### Layers

- `domain::tagging`: `Keywords`, `Change`, `add`, `remove`, `paths`, `suggest`, `path_for`.
- `media::tagging::write` (shares `WriteError`, the sidecar lookup and the date handling with `media::rating`).
- `db::media::mark_for_reading`.
- `app`: `info` (chips with ×, _Add_ field, suggestions), `state::{TagChange, tag}`, `viewer::Action::Tag`, `gallery::TagPaths`.

### Tagging a selection (REQ-026-06)

8. **Rating and tagging a selection share one path:** `state::ChangeMany { ids, what: Many::Rate | Many::Tag }` replaces the rating-only version, with one confirmation dialog, one background job (`media::rating::write` or `media::tagging::write` per file) and one pair of toasts. The selection bar's _Tag_ menu chooses People, Places or Tags and reuses the info panel's field and suggestions (`info::{Group, add_field}`). Only adding: removing a tag from many items is rarer and out of scope.

### Changes to other specs

- **REQ-009-08**: in the desktop app the tag groups offer _Add_ and remove buttons.
- **REQ-004-01** already allows writes the user asks for; spec 026 is added to it.

## Tasks

- [x] `domain::tagging` with tests (REQ-026-01, REQ-026-03)
- [x] `media::tagging::write` with exiftool tests (REQ-026-03, REQ-026-04)
- [x] `db::media::mark_for_reading`
- [x] Info panel and sheet: chips with ×, _Add_ with suggestions; viewer keys while typing; UI tests and snapshot (REQ-026-01 to REQ-026-05)
- [x] Spec changes to 004, 009; glossary, architecture
- [x] Tag the selection: _Tag_ menu, shared batch job, confirmation, toasts (REQ-026-06)

## Decisions

### 2026-11-01 · Tags written by hand, read back by indexing

**Decided:** adding or removing a tag (spec 026) reads the file's keyword fields with a one-shot exiftool, changes the lists in `domain::tagging` and writes back only the fields that changed (`media::tagging`), into the same targets as ratings. A removal also takes the tag out of the sidecar, and its hierarchy's unused levels out of the flat lists. The index isn't written directly: the row is marked for reading (`db::media::mark_for_reading`) and a changes scan of the path reads the file with indexing's own rules. The viewer ignores its shortcuts while a text field has the keyboard, also in the frame after, since egui drops the focus before a frame with Escape.
**Rejected:** writing tags into the index directly (indexing's normalization and one-hierarchy-per-name rules would have to be repeated), appending with exiftool's `+=`/`-=` per field (it can't express "this name in any field, and the hierarchy's levels"), removing names from face regions (other programs' drawings).
