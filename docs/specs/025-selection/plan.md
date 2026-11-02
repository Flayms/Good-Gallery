# 025 · Selection · Plan

## Approach

### Decisions

#31 left two questions open. As proposed on the issue, and decided as such since the user said to go ahead:

1. **A plain click opens the viewer while nothing is selected, and selects in selection mode.** Ctrl-click (or Space on the focused item) starts a selection. Once one exists, a plain click toggles items, as in Google Photos, so the selection isn't lost by accident. Escape or _Clear_ ends it.
2. **A changed search clears the selection; a rescan keeps it.** Otherwise items hidden by a filter would stay selected and an action would change files the user can't see. Items that leave the index leave the selection.
3. **Ctrl+A is "all of the search except these".** `domain::selection::Selection` holds either a set of ids or the ids left out of "all", so selecting 500,000 items is free. Ids are made from the layout (which already holds every id of the search) only when an action runs. Items indexed into the search while all are selected count as selected too.
4. **The anchor for Shift-click is the item last clicked**, also a plain click that opened the viewer; a range is added to the selection (Google Photos), not replacing it (Explorer).
5. **Looks: no brand accent** (`docs/design.md`). A selected tile gets the focus ring's two rings and a round check badge in its top-left corner; in selection mode, a hovered tile shows an empty badge. A List row gets the `selected` background and the badge on its thumbnail. Screen readers hear `, selected` after the item's label.
6. **The toolbar's search bar becomes the selection bar** (`Clear`, `N selected`, _Rate_), since searching while a selection exists would clear it anyway.
7. **Rating a selection** uses the single-item writer (`media::rating::write`, REQ-024-03) for each file on one background thread, updating each index row once its file is written. Above 100 items a dialog asks first. One toast says it started, one how it ended, naming the first failure. Keys `1`–`5` set the rating; unlike a single item they don't toggle, because the selected items may have different ratings. _Rate_ › _Remove rating_ removes it.

### Layers

- `domain::selection`: the model and its rules, tested there (REQ-025-01 to REQ-025-05).
- `db::media::files_of`: the files of a list of ids, in chunks (REQ-025-07).
- `app::gallery`: holds the `Selection`, turns clicks and keys into it (`clicked`, `selection_keys`), clears it on a new search and trims it on a new layout, paints it (`tiles::selected_mark`, `select_hint`, `check_badge_small`).
- `app::toolbar::selection_bar`, `app::state::{ask_rate_many, rate_many}`, `app::lib::confirm_rate_many`.

### Changes to other specs

- **REQ-009-01**: a click opens the viewer only without a selection (REQ-025-03).
- Glossary: _Selection_, _Selection mode_.

## Tasks

- [x] `domain::selection` with tests (REQ-025-01 to REQ-025-05)
- [x] Gallery: clicks, Space, Ctrl+A, Escape, new search, new layout; tiles and rows painted (REQ-025-01 to REQ-025-05)
- [x] Selection bar (REQ-025-06)
- [x] Rating the selection: keys, menu, confirmation, background job, toast (REQ-025-07)
- [x] UI tests and snapshot; spec changes to 009, glossary, architecture

## Decisions

### 2026-11-01 · Selection as "these" or "all except these"

**Decided:** the gallery's selection (spec 025) is `domain::selection::Selection`: a set of ids, or the ids left out of "all of the search", plus the anchor for Shift-click. Ctrl+A on 500,000 items is free; the ids are made from the layout, which already holds every id of the search, only when an action runs. A new search clears the selection and a new layout trims it to the items still there. Rating a selection runs `media::rating::write` per file on one background thread and updates each index row after its file (`db::media::files_of` gives the files).
**Rejected:** selected ids in the index (a write per click, and stale after a crash), materializing all ids on Ctrl+A (megabytes per press on large searches), keeping the selection across searches (hidden items would be changed).
