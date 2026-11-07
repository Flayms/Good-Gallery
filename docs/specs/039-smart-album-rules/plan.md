# 039 · Smart album rules · Plan

## Approach

- **`domain::smart_albums::Rules`** (pure): `match_any: bool`, a flat `Vec<Condition>` and the sort order. `Condition` is an enum with one variant per row of REQ-039-02, each with its comparison and value. A relative date resolves to a day range for a given "today" (REQ-039-04), so tests pin the day.
- **From and to a search:** `Rules::from_search(&GallerySearch)` turns today's filters into `Match all` conditions (REQ-039-01, REQ-039-06). `Rules::as_search()` gives back a `GallerySearch` when every condition is one the filters bar can show and the match is _all_; otherwise the album opens as rules (REQ-039-05).
- **Storage:** the `search` JSON column of `smart_albums` stays. New albums store `{"version": 2, "match": "any", "conditions": [...], "sort": ...}`. A value without `version` is an old search and is read through `from_search`. No migration; old albums are rewritten only when edited and saved.
- **Query:** `db::gallery::filters` gets a sibling, `rule_filters`, that builds one SQL fragment per condition, reusing the fragments the filters already use (tags through `tag_conditions`, the name index, folder ranges), and joins them with `AND` or `OR`. With `OR`, each fragment that needs another table becomes `media.id IN (subquery)`, so SQLite can union index lookups instead of scanning. _Shows_ is the content matches' id set, as today (REQ-036-07). `GallerySearch` gets an optional `rules` field that the loader passes on, so sorting, date groups and the viewer work unchanged.
- **Benchmark:** `cargo xtask search-bench` gets rule albums: five conditions with _any_ over common and rare tags, a relative date, size and orientation (REQ-039-07). If `OR` over the subqueries misses the budget, the fallback is to collect each condition's ids separately and merge them in Rust, as content search already does.
- **New day:** the app already notices date changes for date groups. A new day refreshes the smart album counts and the open gallery when it shows a relative-date album (REQ-039-04).
- **UI:** an editor dialog in the style of the album name dialog: name field, an _all / any_ switch, the condition rows (a condition picker, a comparison, a value field fitting its kind: tag picker with suggestions, day picker, number with unit), _Add condition_, and the sort order. The filters bar shows a chip with the album's name for albums opened as rules.

## Tasks

- [ ] `domain::smart_albums::Rules`, `Condition`, relative dates, from and to a search (REQ-039-02, -03, -04, -06)
- [ ] Stored format version 2, reading old searches (REQ-039-06)
- [ ] `db::gallery::rule_filters` with `AND` / `OR`, counts in the sidebar (REQ-039-03)
- [ ] Benchmark rule albums in `search-bench`; merge in Rust if `OR` misses the budget (REQ-039-07)
- [ ] Refresh relative-date albums on a new day (REQ-039-04)
- [ ] Editor dialog, _Edit…_ in the album's menu, _Save as smart album…_ opening it (REQ-039-01, -02)
- [ ] The album chip in the filters bar (REQ-039-05)

## Decisions

### 2026-11-07 · A flat list

**Decided:** one list of conditions with _all_ or _any_, as agreed in #203.
**Rejected:** nested groups, which most people get wrong and which SQLite plans badly.

### 2026-11-07 · Rules only in smart albums

**Decided:** the filters bar keeps the filters it has. An album with rules it can't show opens with a chip of its name.
**Rejected:** every condition in the filters bar too. It would crowd the bar, and it can be added later on top of the same `Condition` type.
