# 035 · Rename many files

**Status:** Approved

## Goal

Photos arrive with names like `IMG_4711.JPG` or `PXL_20230512_101500123.jpg`, and a library mixes the conventions of every camera and phone the user had. Renaming them one by one in a file manager is tedious, and other tools don't know the dates and tags the index already has. The user renames a selection at once, with search and replace or a pattern built from the photos' own metadata, sees every result before anything happens, and can undo it. Nothing the app knows about a file is lost by renaming it.

## Requirements

### REQ-035-01 · Rename a selection

- **Given** a selection in the desktop app
- **When** the user chooses _Rename…_ in the selection bar or the selection's context menu
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
- **When** the user chooses _Undo rename_ in the toast, or in the selection bar's menu until the next rename
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
| REQ-035-01 | – | no test |
| REQ-035-02 | – | no test |
| REQ-035-03 | – | no test |
| REQ-035-04 | – | no test |
| REQ-035-05 | – | no test |
| REQ-035-06 | – | no test |
| REQ-035-07 | – | no test |
| REQ-035-08 | – | no test |
| REQ-035-09 | – | no test |
