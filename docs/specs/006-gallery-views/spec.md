# 006 · Gallery views

**Status:** Approved

## Goal

The user scrolls through hundreds of thousands of media items as smoothly as through a few hundred, in the layout that suits the content: photos at their natural aspect ratio (Masonry, Justified), a uniform overview (Grid) or file details (List). Tile size is adjustable, and the gallery reflects indexing progress live.

## Requirements

### REQ-006-01 · Choose a gallery view

- **Given** the gallery
- **When** the user picks Masonry, Justified, Grid or List under _View_ in the toolbar (013)
- **Then** the gallery shows the same media items in the same order in that view
- **And** the choice is remembered across sessions (default: Masonry), and clicking the active view keeps it

### REQ-006-02 · Masonry view

- **Given** the Masonry view
- **When** it is shown
- **Then** tiles are placed in columns of equal width, each tile into the currently shortest column, with an 8 px gap
- **And** each tile keeps its media item's aspect ratio, limited to between 3:1 landscape and 1:3 portrait (cropped beyond that), and is square if the dimensions are unknown

### REQ-006-03 · Justified view

- **Given** the Justified view
- **When** it is shown
- **Then** tiles are placed in rows that fill the full width exactly, all tiles of a row equally tall and at their aspect ratio (limited as in REQ-006-02)
- **And** rows are about as tall as 3/4 of a Masonry column at the same zoom level, the last row isn't stretched, and an item too wide for the target height gets a row of its own

### REQ-006-04 · Grid view

- **Given** the Grid view
- **When** it is shown
- **Then** tiles are equal squares in columns, with the thumbnail cropped to fill them

### REQ-006-05 · List view

- **Given** the List view
- **When** it is shown
- **Then** each media item is a row with a small thumbnail, file name (videos with a play icon and duration), folder, date, size, dimensions and rating stars, under a header that stays visible while scrolling
- **And** the date is the date taken. Without one, the date modified is shown dimmed and its tooltip says so

### REQ-006-06 · Zoom

- **Given** the Masonry, Justified or Grid view
- **When** the user moves the zoom slider or clicks _Zoom in_ / _Zoom out_ under _View_, or turns the mouse wheel while holding Ctrl over the gallery
- **Then** the tiles grow or shrink in steps between 12 and 2 columns (default 5), one step per wheel notch, with small touchpad movements adding up to steps and a change of direction starting over
- **And** the zoom level is remembered across sessions, and the zoom controls are disabled in the List view

### REQ-006-07 · Keep the scroll position

- **Given** the gallery scrolled to some item
- **When** the zoom level or the window size changes
- **Then** the item that was first visible stays in view

### REQ-006-08 · Large galleries

- **Given** a search matching hundreds of thousands of media items
- **When** the gallery shows it
- **Then** the scrollbar covers the whole result right away, so the user can jump anywhere, and scrolling stays smooth
- **And** item details load only for the visible range. Until then, positions show a gray placeholder

### REQ-006-09 · Video tiles

- **Given** a video tile
- **When** it is shown
- **Then** it shows a play badge with the duration (`m:ss`, or `h:mm:ss` from one hour on)

### REQ-006-10 · Empty, loading and error states

- **Given** the gallery
- **When** there are no libraries yet / nothing matches the search / the media items are still loading / loading failed
- **Then** it shows "No libraries yet" with an _Add a library_ link to Settings / "No media found" with a hint that new files appear while indexing / skeleton tiles or rows matching the view / "Couldn't load media" with the error message

### REQ-006-11 · Live updates while indexing

- **Given** a running scan
- **When** it writes media items to the index
- **Then** the gallery, sidebar counts and tag suggestions refresh right after the first written files, then at most every 5 seconds, and once more when the scan ends

### REQ-006-12 · Frame-time budget

- **Given** a gallery of 100,000 media items at 12 columns, with their thumbnails in the thumbnail cache
- **When** the user scrolls continuously
- **Then** the 95th percentile of frame times stays below 16 ms on an integrated laptop GPU (measured without vsync)

### REQ-006-13 · Thumbnail memory budget

- **Given** the user scrolls through a large gallery
- **When** thumbnails are loaded for display
- **Then** the memory they take on the GPU stays within a fixed budget, dropping the least recently shown first
- **And** thumbnails of visible tiles and of tiles about to scroll into view are kept

## Out of scope

- Selecting multiple media items, and drag and drop to other apps.
- Grouping by folder (section headers). Grouping by day is in 016.
- Custom column sets or sorting by clicking list headers (sorting is in 007).
- A setting for the gap size or tile corner radius.

## Assumptions

- Width and height in the index are the displayed dimensions (REQ-004-02), so layouts can be computed without loading any image.
- A search result fits in memory as a list of ids and aspect ratios (about 500k entries).

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| REQ-006-01 | — | no test |
| REQ-006-02 | — | no test |
| REQ-006-03 | — | no test |
| REQ-006-04 | — | no test |
| REQ-006-05 | — | no test |
| REQ-006-06 | — | no test |
| REQ-006-07 | — | no test |
| REQ-006-08 | — | no test |
| REQ-006-09 | — | no test |
| REQ-006-10 | — | no test |
| REQ-006-11 | — | no test |
| REQ-006-12 | — | no test |
| REQ-006-13 | — | no test |
