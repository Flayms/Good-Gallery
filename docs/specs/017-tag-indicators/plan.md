# 017 · Tag indicators · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Data:** `db::gallery::items` returns each item's tag categories (People, Places, other tags) with its rating; `domain::view::indicators_of` turns them and the picked indicators into a fixed order.
- **Badge:** a solid `rgb(0 0 0 / 0.6)` badge in the tile's bottom-left corner with Star-colored stars and white pin, person and tag icons, clipped so it never covers the video badge. Each indicator has a tooltip, and the tile's accessible name lists them (REQ-017-02).
- **Choice** (REQ-017-03): a switch and Rating / Places / People / Tags toggles in the _View_ menu, stored as `gallery.tagIndicators` (`{"shown": …, "picked": […]}`); off until turned on, all four picked by default; disabled in the List view.

## Tasks

- [x] Indicators per item in a fixed order (REQ-017-01)
- [x] Badge with tooltips and accessible names (REQ-017-02)
- [x] Choice in the _View_ menu, remembered (REQ-017-03)
