# 008 · Sidebar

**Status:** Approved (ported from the reference, not yet implemented in Rust)

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

## Out of scope

- Configuring which tag names count as People or Places roots, or adding categories.
- Library-specific counts in the People, Places, Ratings and Tags groups (they count across all libraries).
- Showing counts that reflect the current filters.
- Drag and drop or context menus in the sidebar.

## Assumptions

- Photo tools write people and places as keyword hierarchies under a top-level tag with one of the root names above (digiKam, Lightroom and others do). Face regions always end up under `People` (REQ-004-09).
- Libraries have a manageable number of folders. The folder tree is only loaded for the selected library.

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| REQ-008-01 | — | no test |
| REQ-008-02 | — | no test |
| REQ-008-03 | — | no test |
| REQ-008-04 | — | no test |
| REQ-008-05 | — | no test |
| REQ-008-06 | — | no test |
| REQ-008-07 | — | no test |
