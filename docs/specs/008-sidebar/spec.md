# 008 · Sidebar

**Status:** Implemented

## Goal

The user browses by structure rather than by typing: libraries and their folders, the people and places their photo tools already tagged, ratings and the most used tags. All of it with counts, one click away from filtering the gallery.

## Requirements

### REQ-008-01 · Libraries

- **Given** one or more libraries
- **When** the sidebar is shown
- **Then** it lists _All libraries_ followed by each library sorted by label, with its path as tooltip, and highlights the active one
- **And** choosing a library limits the gallery to it (clearing any folder) while keeping the other filters. _All libraries_ removes the limit
- **And** an offline library shows an unplugged icon and an "offline" badge, and a library being scanned shows a spinner instead

### REQ-008-02 · Folder tree

- **Given** a selected library
- **When** the sidebar is shown
- **Then** the library's folders that contain media items, and the folders leading to them, appear as a tree below it, sorted naturally (`2` before `10`, ignoring letter case)
- **And** each folder's tooltip shows its path and the number of media items in it and its subfolders
- **And** choosing a folder limits the gallery to it and its subfolders and expands it, the chevron expands or collapses it, and the folders leading to the active folder start expanded

### REQ-008-03 · People and Places

- **Given** tags below a category root (`People`, `Person`, `Persons`, `Faces` for People. `Places`, `Place`, `Locations`, `Location` for Places, in any letter case)
- **When** the sidebar is shown
- **Then** a _People_ and a _Places_ group list those tags as a tree without the root itself (e.g. country › city), most used first, with counts that include descendants
- **And** a group shows its 10 most used top-level tags, plus _Show all (n)_ / _Show less_. Collapsing the group returns it to the 10
- **And** a group without tags is hidden

### REQ-008-04 · Ratings

- **Given** rated media items
- **When** the sidebar is shown
- **Then** a _Ratings_ group lists each rating that occurs (5 stars down to _Unrated_) with its number of media items across all libraries
- **And** clicking a rating adds it to the rating filter, or removes it if it's already there
- **And** the group is hidden while all media items are unrated

### REQ-008-05 · Tags

- **Given** tags outside the People and Places categories
- **When** the sidebar is shown
- **Then** a _Tags_ group lists the 15 most used of them with counts that include descendants, and is hidden if there are none

### REQ-008-06 · Filter by clicking a tag

- **Given** a tag in the People, Places or Tags group
- **When** the user clicks it
- **Then** it is added to the search as an included tag, or removed from the search if it was included or excluded
- **And** included tags are highlighted, excluded tags show a minus icon

### REQ-008-07 · Remember collapsed groups

- **Given** the collapsible People, Places and Ratings groups
- **When** the user collapses or expands one
- **Then** that state is kept across sessions

### REQ-008-08 · Hide and show the sidebar

- **Given** the sidebar is shown
- **When** the user clicks _Hide sidebar_ in its header, or presses Ctrl + B while the viewer is closed
- **Then** the sidebar hides and the gallery or the Settings page takes its width, and a _Show sidebar_ button appears at the left of the toolbar and of the Settings header
- **And** clicking _Show sidebar_, or pressing Ctrl + B again, shows the sidebar where it was (same groups, scroll position and filters)

### REQ-008-09 · Remember the hidden sidebar

- **Given** the user hid or showed the sidebar
- **When** the app starts the next time
- **Then** the sidebar is hidden or shown as it was

### REQ-008-10 · Order and choose the groups

- **Given** the _Sidebar_ section of the Settings page, which lists the People, Places, Ratings and Tags groups in their current order, each with a switch and _Move up_ / _Move down_ buttons
- **When** the user moves a group up or down, or switches it off or on
- **Then** the sidebar shows the switched-on groups in that order below the libraries, from the next frame on
- **And** the libraries stay first and can't be hidden, and _Settings_ stays at the bottom
- **And** a switched-on group without entries is still hidden (REQ-008-03, REQ-008-04, REQ-008-05)

### REQ-008-11 · Remember and reset the groups

- **Given** the user reordered or switched off sidebar groups
- **When** the app starts the next time
- **Then** the sidebar shows the same groups in the same order, for all libraries alike
- **And** _Reset to default_ in the _Sidebar_ section brings back People, Places, Ratings, Tags, all switched on. It is disabled while that is already the case
- **And** a stored choice that lacks a group (e.g. one added by an update) shows that group switched on at the end, and unknown or repeated entries are ignored

## Out of scope

- Configuring which tag names count as People or Places roots, or adding categories.
- Library-specific counts in the People, Places, Ratings and Tags groups (they count across all libraries).
- Showing counts that reflect the current filters.
- Drag and drop or context menus in the sidebar, or dragging the groups in the settings.
- Moving or hiding the libraries and folder tree, or a different group order per library.
- Resizing the sidebar, or hiding it automatically in narrow windows.

## Assumptions

- Photo tools write people and places as keyword hierarchies under a top-level tag with one of the root names above (digiKam, Lightroom and others do). Face regions always end up under `People` (REQ-004-09).
- Libraries have a manageable number of folders. The folder tree is only loaded for the selected library.

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| REQ-008-01 | `crates/app/tests/search.rs` › req_008_01_req_008_02_req_007_10_limits_the_gallery_to_a_library_and_its_folders | pass (the spinner while scanned checked in the app) |
| REQ-008-02 | `crates/domain/src/folders.rs` › req_008_02_adds_intermediate_folders_sums_counts_and_sorts_naturally, req_008_02_sorts_naturally_ignoring_letter_case, req_008_02_sorts_accented_names_with_their_base_letters, req_008_02_lists_the_path_and_its_ancestors; `crates/db/src/tags.rs` › req_008_02_lists_folders_with_direct_media_counts; `crates/app/tests/search.rs` › req_008_01_req_008_02_req_007_10_limits_the_gallery_to_a_library_and_its_folders | pass |
| REQ-008-03 | `crates/domain/src/folders.rs` › req_008_03_nests_tags_under_their_parent_in_the_given_order, req_008_03_keeps_tags_whose_parents_form_a_cycle; `crates/db/src/tags.rs` › req_008_03_groups_by_the_top_level_tag_whatever_its_exact_name_with_counts_including_descendants, req_008_03_keeps_the_hierarchy_below_the_root; `crates/app/tests/search.rs` › req_008_03_req_008_06_filters_by_clicking_people_places_and_tags (snapshot `sidebar`) | pass (Show all / Show less checked in the app) |
| REQ-008-04 | `crates/db/src/tags.rs` › req_008_04_counts_media_per_rating_0_meaning_unrated; `crates/app/tests/search.rs` › req_008_04_req_008_07_toggles_ratings_and_remembers_collapsed_groups | pass |
| REQ-008-05 | `crates/db/src/tags.rs` › req_008_05_excludes_category_tags_and_their_roots; `crates/app/tests/search.rs` › req_008_03_req_008_06_filters_by_clicking_people_places_and_tags | pass |
| REQ-008-06 | `crates/domain/src/search.rs` › req_007_05_req_008_06_adds_moves_and_removes_tags_case_insensitively; `crates/app/tests/search.rs` › req_008_03_req_008_06_filters_by_clicking_people_places_and_tags | pass |
| REQ-008-07 | `crates/app/tests/search.rs` › req_008_04_req_008_07_toggles_ratings_and_remembers_collapsed_groups | pass |
| REQ-008-08 | `crates/app/tests/search.rs` › req_008_08_req_008_09_hides_and_shows_the_sidebar_and_remembers_it | pass |
| REQ-008-09 | `crates/app/tests/search.rs` › req_008_08_req_008_09_hides_and_shows_the_sidebar_and_remembers_it | pass |
| REQ-008-10 | `crates/domain/src/sidebar.rs` › req_008_10_moves_groups_and_switches_them_off; `crates/app/tests/search.rs` › req_008_10_req_008_11_orders_and_hides_groups_from_the_settings_and_remembers_them (snapshot `settings_sidebar`) | pass |
| REQ-008-11 | `crates/domain/src/sidebar.rs` › req_008_11_reads_back_what_it_stores_and_repairs_odd_values, req_008_11_resets_to_the_default; `crates/app/tests/search.rs` › req_008_10_req_008_11_orders_and_hides_groups_from_the_settings_and_remembers_them | pass |
