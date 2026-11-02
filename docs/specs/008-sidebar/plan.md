# 008 · Sidebar · Plan

Records the approach that was taken. Lasting decisions are under _Decisions_ at the end.

## Approach

- **Trees** (`domain::folders`): `build_folder_tree` from folders with direct counts (intermediate folders added, counts summed, natural order: numbers by value, ignoring case and accents), `folder_ancestors`; `build_tag_tree` from a category's flat list (tags whose parents form a cycle still appear).
- **Queries** (`db::tags`): People and Places by top-level ancestor (whatever the root's exact name), counts including descendants; popular tags without category tags and their roots; rating counts (0 = unrated); a library's folders with direct counts.
- **Sidebar** (`app::sidebar`): a fixed 256 px left panel on Recess. _All libraries_ and each library (path as tooltip, offline badge, spinner while scanned), the selected library's folder tree (ancestors of the active folder expanded), _People_ and _Places_ trees (10 top-level tags, _Show all (n)_ / _Show less_), _Ratings_ (5 … Unrated with counts), _Tags_ (15 most used). Clicking filters; included tags are highlighted, excluded ones marked. Settings sits at the bottom. Rows are drawn by one helper.
- **Data** loads on the gallery's loader thread and reloads with the gallery, only when it can change (not on every filter click). Collapsed groups are stored as `sidebar.group.<id>` in the `settings` table.

## Tasks

- [x] Folder tree and tag trees (REQ-008-02, -03)
- [x] Category, popular, rating and folder queries (REQ-008-02 … -05)
- [x] Libraries and folder tree in the sidebar (REQ-008-01, -02)
- [x] People, Places, Ratings, Tags; clicking tags; collapsed groups remembered (REQ-008-03 … -07)

## Decisions

### 2026-10-27 · Search bar, filters and sidebar

**Decided:** the gallery owns the search (`Gallery::set_search` reloads from the top). _Filters_ and _View_ are `egui::Popup`s under fixed ids: a button's automatic id changes with its look and with the widgets drawn before it, which closed the menus. The sidebar is a fixed 256 px left panel; collapsed groups are stored as `sidebar.group.<id>` in `settings`. The groups' order and which of them show (`domain::sidebar::SidebarLayout`) are stored once for the app as `sidebar.groups`, a list of `{group, shown}`; reading it repairs what an older or newer version left behind instead of rejecting it.
