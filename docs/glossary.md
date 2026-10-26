# Glossary

Domain terms used in specs, code and UI. Code names differ from the UI term in some places. Those names are given in parentheses.

## Library and files

| Term                     | Definition                                                                                                                                                                        |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |

| **Library**              | A folder on a local disk or a network share (`\\server\share`) that the user added. Indexed recursively. (code: _library root_, `library_roots`, `rootId`)                          |
| **Library status**       | `online` (reachable), `offline` (unreachable at the last check) or `unknown`. The index of an offline library stays browsable.                                                      |
| **Media item**           | One indexed image or video file in a library. (code: `media`)                                                                                                                     |
| **Media kind**           | `image` (JPEG, PNG, WebP, GIF, AVIF) or `video` (MP4, M4V, MOV, WebM, MKV). Shown as _Photos_ / _Videos_ in the UI.                                                                 |
| **Sidecar**              | An `.xmp` file next to a media file (`IMG_1.jpg.xmp` or `IMG_1.xmp`) that holds additional metadata.                                                                               |
| **Folder**               | A subfolder of a library, addressed by its `/`-separated path relative to the library. A folder filter includes its subfolders.                                                     |
| **Housekeeping folder**  | A folder created by an OS or NAS that is never indexed: dot folders, `$` folders, `@eaDir`, `#recycle`, `#snapshot`, `.@__thumb`, `System Volume Information`.                      |
| **Read-only**            | Good Gallery never modifies media files or sidecars. All metadata comes from what other tools wrote into them.                                                                    |

## Metadata

| Term                  | Definition                                                                                                                                                       |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Metadata**          | The dimensions, date taken, duration, rating and tags of a media item, read from embedded EXIF / IPTC / XMP and its sidecar.                                     |
| **Date taken**        | The capture date from the metadata. If it is missing, sorting and the date filter use the _date modified_ instead. (code: `takenAt`, fallback `sortDate`)            |
| **Date modified**     | The file's last modification time. (code: `mtime`)                                                                                                               |
| **Rating**            | 1–5 stars from the metadata. Media without a rating, or with a _rejected_ (-1) or 0 rating, are **unrated**. Filters use `0` for unrated.                        |
| **Tag**               | A keyword from the metadata. Tags are compared case-insensitively after Unicode (NFKC) and whitespace normalization. The first spelling seen is displayed.       |
| **Tag hierarchy**     | Tags nested as `Parent\|Child\|Leaf` (e.g. `Places\|France\|Paris`). A media item is linked to the leaf. Searching for a tag also matches its descendants.        |
| **Face region**       | A named area of a photo marked as a face by a photo app. Its name becomes the tag `People\|<name>`.                                                               |
| **Tag category**      | _People_ or _Places_. A tag belongs to a category when its top-level ancestor is a **category root**: `People`, `Person`, `Persons`, `Faces` / `Places`, `Place`, `Locations`, `Location`. |
| **Metadata version**  | The version of the metadata extraction rules. Media indexed with an older version are read again. (code: `METADATA_VERSION`, `media.meta_version`)                  |

## Indexing

| Term                 | Definition                                                                                                                                                                   |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Index**            | The local database of libraries, media items, folders and tags. Lives in the user profile, separate from the libraries.                                                         |
| **Indexer**          | The background threads that scan libraries, reads metadata and renders thumbnails.                                                                                          |
| **Scan**             | One sync of a library with the index. At most one scan runs at a time. Further scans wait in the **scan queue**, one per library.                                              |
| **Full scan**        | Lists every folder and checks every file. Runs when a library is added, on manual _Rescan_, and once after a metadata version change. Shown as _Indexing_.                       |
| **Quick scan**       | Only lists folders whose contents (added, removed or renamed entries) changed since the last scan. Runs at startup and periodically. Shown as _Checking_.                    |
| **Changes scan**     | Checks only the paths that the watcher reported.                                                                                                                             |
| **Watcher**          | Watches a library for file changes between scans. Starts after a completed scan.                                                                                             |
| **Quick check interval** | How often all libraries get a quick scan. `0` means at startup only. (Setting: _Check for missed changes_)                                                                |
| **Parallel file reads** | How many files the indexer reads at once (scanning, metadata, thumbnails). (code: `ioConcurrency`)                                                                         |

## Thumbnails

| Term                    | Definition                                                                                                                                                  |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Thumbnail**           | A downscaled WebP image of a media item, 400 or 800 px wide. A video's thumbnail is a frame at 10 % of its duration.                                           |
| **Placeholder**         | A tiny blurred preview (ThumbHash) stored in the index. It is shown until the thumbnail has loaded.                                                         |
| **Thumbnail cache**     | Local, size-capped folder of thumbnails. When the cap is exceeded, the least recently viewed thumbnails are removed first.                                       |
| **On-demand rendering** | Rendering a thumbnail because a tile needs it now. Takes priority over **background rendering**, which renders thumbnails of all media items, newest first. |
| **Failed thumbnail**    | A thumbnail that couldn't be rendered from a reachable file. Not retried until the file changes.                                                            |

## Browsing

| Term              | Definition                                                                                                                                                       |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Gallery**       | The scrollable area showing all media items that match the current search, in sort order.                                                                         |
| **Gallery view**  | How the gallery is laid out: **Masonry** (columns, tiles keep their aspect ratio), **Justified** (rows of equal height), **Grid** (square tiles), **List** (rows with details). |
| **Date group**    | The media items of one local calendar day of the sort date, under a header with the day and the number of photos and videos. Only when sorted by a date. (UI: _Group by day_) |
| **Tile**          | One media item in the Masonry, Justified or Grid view.                                                                                                           |
| **Tag indicators** | Optional icons in a tile's corner: one star per rating point, and icons when the media item has _Places_, _People_ or other tags. The user picks which are shown. |
| **Zoom level**    | The tile size of the gallery as a number of columns (2–12). More columns means smaller tiles. (code: `columns`)                                                  |
| **Search**        | The filters and sort order of the gallery: tags, file name, library, folder, media kind, date range, rating. Kept while the viewer is open. (code: `GallerySearch`)            |
| **Included / excluded tag** | A tag that matching media items must have (including descendants), or must not have.                                                             |
| **Search bar**    | The toolbar input with chips for included / excluded tags and the file name filter, plus tag suggestions.                                                       |
| **Sidebar**       | Navigation on the left: libraries with their folder tree, People, Places, Ratings, Tags, indexer progress and Settings.                                          |
| **Viewer**        | The full-window overlay that shows one media item in original quality. (also: lightbox)                                                                          |
| **Filmstrip**     | The optional row of thumbnails of the whole gallery below the media item in the viewer, with the shown item highlighted.         |
| **Info panel**    | The panel in the viewer with file details, rating and tags. It opens as the **info sheet** when used from the gallery's context menu.                              |
