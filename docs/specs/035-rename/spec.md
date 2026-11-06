# 035 · Rename many files

**Status:** Implemented

## Goal

Photos arrive with names like `IMG_4711.JPG` or `PXL_20230512_101500123.jpg`, and a library mixes the conventions of every camera and phone the user had. Renaming them one by one in a file manager is tedious, and other tools don't know the dates and tags the index already has. The user renames a selection at once, with search and replace or a pattern built from the photos' own metadata, sees every result before anything happens, and can undo it. Nothing the app knows about a file is lost by renaming it.

## Requirements

### REQ-035-01 · Rename a selection

- **Given** a selection in the desktop app
- **When** the user chooses _Rename…_ in the selection bar
- **Then** a dialog opens for the selected media items
- **And** the browser version doesn't offer it

### REQ-035-02 · Search and replace

- **Given** the rename dialog
- **When** the user enters what to find and what to put instead, as plain text or as a regular expression, with or without matching case
- **Then** every match in a file name is replaced, and a regular expression's groups can be used in the replacement (`$1`)
- **And** an empty search replaces the whole name, so the replacement alone makes the new name
- **And** only the name changes, never the extension

### REQ-035-03 · Placeholders

- **Given** a replacement with placeholders
- **When** the new names are worked out
- **Then** `{name}` is the old name, `{taken:FORMAT}` the date taken, `{modified:FORMAT}` the date modified (`FORMAT` as `yyyy-MM-dd HH-mm-ss`, any part optional), and `{tags}` the media item's tags without People and Places, joined with `, `
- **And** a placeholder the media item has no value for makes its row fail (REQ-035-05)

### REQ-035-04 · Preview

- **Given** the rename dialog
- **When** anything in it changes
- **Then** a list shows each selected media item with its old and new name, changed rows marked, and the number of changed, unchanged and failing rows above it
- **And** it stays responsive for 50,000 media items

### REQ-035-05 · Names that can't be used

- **Given** a new name that would be a second file of the same name in its folder (in the batch or already on disk), contains a character Windows forbids or a control character, ends with a space or a dot, is a name Windows reserves (`CON`, `NUL`, …), is empty, or makes a path longer than Windows allows
- **When** the preview shows
- **Then** its row says why, and it is left out, while the other rows can still be renamed

### REQ-035-06 · Renaming

- **Given** a preview with changed rows
- **When** the user chooses _Rename_
- **Then** the files are renamed in the background with their sidecars (`IMG_1.jpg.xmp` and `IMG_1.xmp`), and the dialog closes
- **And** names swapped within the batch (`a → b`, `b → a`) and changes only in case work, also on Windows
- **And** a file that can't be renamed keeps its name, the rest carry on, and a toast counts them and names the first with the reason

### REQ-035-07 · Nothing is lost

- **Given** a renamed media item
- **When** the app or its watcher notices the change
- **Then** it is the same media item: its rating, tags, faces and face decisions, edit (REQ-021-11) and place in the selection stay, and its thumbnail isn't rendered again
- **And** the unedited original of an edited photo (REQ-021-10) is renamed with it, and an edited copy still finds its original after that original was renamed

### REQ-035-08 · Undo

- **Given** a rename that finished
- **When** the user chooses _Undo_ in the toast that says the files were renamed, or _Undo last rename_ in the rename dialog until the next rename
- **Then** every file renamed by it gets its old name back, as long as nothing else took that name meanwhile
- **And** this also works after the app was closed

### REQ-035-09 · Interrupted

- **Given** a rename that was interrupted (the app closed or crashed, the share went offline)
- **When** the app starts again, or the share is back
- **Then** the index matches the files: each media item has the name its file has

## Out of scope

- Fixing dates (#206) and saving a rename to apply again (#207).
- Moving files to other folders, and renaming folders.
- Renaming in the browser version, and files outside libraries (spec 033).
- Changing the extension or its case.

## Assumptions

- The user can write to the library's folders; on a share that refuses, REQ-035-06's toast says so for each file.
- A rename within a folder doesn't change a file's size or date modified, on local disks and on SMB shares.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-035-01 | `crates/app/tests/rename.rs` › req_035_01_req_035_02_req_035_04_opens_for_the_selection_and_previews_the_names (snapshot `rename_dialog`) | pass |
| REQ-035-02 | `crates/domain/src/rename.rs` › req_035_02_replaces_plain_text_with_or_without_case_and_keeps_the_extension, req_035_02_uses_groups_of_a_regular_expression_and_an_empty_search_replaces_the_name; `crates/app/tests/rename.rs` › req_035_01_req_035_02_req_035_04_opens_for_the_selection_and_previews_the_names | pass |
| REQ-035-03 | `crates/domain/src/rename.rs` › req_035_03_fills_in_placeholders; `crates/db/src/rename.rs` › req_035_03_reads_items_with_their_tags_outside_people_and_places | pass |
| REQ-035-04 | `crates/domain/src/rename.rs` › req_035_04_req_035_05_previews_every_row_and_leaves_out_the_ones_that_cant_work, req_035_04_previews_50000_items_quickly; `crates/app/tests/rename.rs` › req_035_01_req_035_02_req_035_04_opens_for_the_selection_and_previews_the_names | pass |
| REQ-035-05 | `crates/domain/src/rename.rs` › req_035_05_refuses_names_windows_cant_use, req_035_04_req_035_05_previews_every_row_and_leaves_out_the_ones_that_cant_work, req_035_05_names_freed_in_the_batch_can_be_taken_and_names_kept_cant | pass |
| REQ-035-06 | `crates/domain/src/rename.rs` › req_035_06_orders_swaps_chains_and_case_changes_through_temporary_names; `crates/media/src/rename.rs` › req_035_06_req_035_07_moves_sidecars_and_the_backup_along, req_035_06_swaps_and_changes_case_and_a_failure_costs_only_its_file; `crates/app/tests/rename.rs` › req_035_06_req_035_07_req_035_08_renames_keeps_the_media_items_and_undoes | pass |
| REQ-035-07 | `crates/db/src/rename.rs` › req_035_07_renames_rows_in_place_also_swapped_and_moves_edits_along; `crates/indexer/src/thumb_cache.rs` › req_035_07_moves_a_renamed_files_thumbnails; `crates/indexer/src/scan.rs` › req_035_07_a_rename_by_the_app_keeps_the_media_item; `crates/app/tests/rename.rs` › req_035_06_req_035_07_req_035_08_renames_keeps_the_media_items_and_undoes | pass |
| REQ-035-08 | `crates/media/src/rename.rs` › req_035_08_req_035_09_the_journal_records_what_happened; `crates/app/tests/rename.rs` › req_035_06_req_035_07_req_035_08_renames_keeps_the_media_items_and_undoes | pass |
| REQ-035-09 | `crates/media/src/rename.rs` › req_035_08_req_035_09_the_journal_records_what_happened; `crates/app/tests/rename.rs` › req_035_09_finishes_an_interrupted_rename | partial (when a share comes back without a restart: not built, see plan) |
