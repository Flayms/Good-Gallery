# 021 · Photo editing

**Status:** Draft

## Goal

Users who find a photo that is too dark, has a colour cast or lacks punch fix it in the viewer, without exporting it to another program. They adjust light, curves, colour and a few effects with live feedback, then either keep the original and save an edited copy, or replace the original. Either way the edit can be taken back: the unedited original is never lost, and reopening an edited photo shows the sliders where they were left. Colours, bit depth and metadata survive the round trip.

## Requirements

### REQ-021-01 · Edit a photo

- **Given** a JPEG, PNG or WebP photo open in the viewer, in a library that is online
- **When** the user chooses _Edit_ (control bar button, `E`, or the context menu)
- **Then** the viewer switches to editing: the photo stays in view and an adjustments panel replaces the info panel
- **And** videos, GIFs, AVIF photos and media of offline libraries offer no _Edit_

### REQ-021-02 · Live preview

- **Given** editing
- **When** the user changes an adjustment
- **Then** the photo shows the result from the next frame on, at any zoom level, without the file being touched

### REQ-021-03 · Light

- **Given** editing
- **When** the user changes _Exposure_ (−5 to +5 EV), _Contrast_, _Highlights_, _Shadows_, _Whites_ or _Blacks_ (each −100 to +100)
- **Then** exposure scales the light by 2^EV; contrast steepens or flattens the tones around the midtone; highlights and shadows lift or lower only the bright or dark tones; whites and blacks move the white and black points
- **And** each at 0 leaves the photo unchanged

### REQ-021-04 · Curves

- **Given** editing
- **When** the user adds, drags or removes points on the tone curve, for all channels together or for red, green or blue alone
- **Then** the tones are mapped through a smooth, monotonic curve through those points
- **And** a straight curve from black to white leaves the photo unchanged

### REQ-021-05 · Colour

- **Given** editing
- **When** the user changes _Temperature_, _Tint_, _Vibrance_ or _Saturation_ (each −100 to +100), or picks a neutral point with the white balance picker
- **Then** temperature shifts the colours towards blue or yellow, tint towards green or magenta; saturation scales all colours alike, vibrance mostly the muted ones and spares skin tones; the picker sets temperature and tint so the picked spot turns neutral grey
- **And** each at 0 leaves the photo unchanged

### REQ-021-06 · Effects

- **Given** editing
- **When** the user changes _Grain_ (amount, size) or _Vignette_ (amount −100 to +100, midpoint)
- **Then** grain adds film-like noise, a negative vignette darkens and a positive one lightens the edges
- **And** grain looks the same in the preview and in the saved file, and the same each time the photo is saved

### REQ-021-07 · Compare, reset and undo

- **Given** editing
- **When** the user holds `\` (or the _Compare_ button), double-clicks a slider, chooses _Reset all_, or presses `Ctrl+Z` / `Ctrl+Shift+Z`
- **Then** the original shows while held; that adjustment returns to 0; all adjustments return to 0; the last change is undone or redone

### REQ-021-08 · Leave without saving

- **Given** editing with changes that were not saved
- **When** the user presses `Esc`, steps to another item or closes the viewer
- **Then** a dialog offers _Save_, _Discard_ and _Cancel_; without changes, editing just ends

### REQ-021-09 · Save as copy

- **Given** editing with changes
- **When** the user chooses _Save as copy_
- **Then** a new file `<name> (edited).<ext>` (numbered if taken) is written next to the original, and the original and its sidecar stay unchanged
- **And** the copy appears in the gallery next to the original, and the viewer shows it

### REQ-021-10 · Save over the original

- **Given** editing with changes
- **When** the user chooses _Save_, and confirms the first time a photo is replaced
- **Then** the unedited original is first kept as a backup, then the file is replaced with the edited photo
- **And** the gallery's thumbnail and the viewer show the edited photo

### REQ-021-11 · Edits stay editable

- **Given** a photo that was saved with an edit (over the original or as a copy) whose unedited original still exists
- **When** the user edits it again
- **Then** the sliders and curves start where they were saved, and the result is rendered from the unedited original, so repeated edits never stack or lose quality

### REQ-021-12 · Revert to original

- **Given** a photo that was saved over its original
- **When** the user chooses _Revert to original_ in the info panel or the context menu
- **Then** the file is the unedited original again, byte for byte, and the backup is removed
- **And** a copy saved with _Save as copy_ has no _Revert_; the original is still there, and the copy can be deleted with other tools

### REQ-021-13 · Faithful colours and depth

- **Given** a photo with an embedded colour profile, or a PNG with 16 bits per channel
- **When** it is edited and saved
- **Then** the saved file keeps the colour profile and the bit depth, and adjustments at 0 give the same colours as the original (within one level per channel)
- **And** smooth gradients don't band (no 8-bit intermediate steps)

### REQ-021-14 · Metadata kept

- **Given** a photo with metadata (dates, rating, tags, face regions, GPS)
- **When** it is saved as a copy or over the original
- **Then** the saved file carries the same metadata as the original, and its orientation is upright in the pixels (orientation tag 1)
- **And** a copy gets a copy of the original's sidecar under its own name, so it has the same rating and tags

### REQ-021-15 · Date modified on save

- **Given** the setting _When saving edits_ with _Keep date modified_ (default) or _Update date modified_
- **When** an edit is saved (as a copy or over the original), or an original is reverted
- **Then** the file's date modified is the original's, or the time of saving
- **And** the date taken never changes

### REQ-021-16 · Safe writes

- **Given** saving or reverting
- **When** the library goes offline, the folder is read-only, the disk is full or the app is ended part-way
- **Then** either the old or the new file is in place, never a partial one, the backup is never lost, and a failure shows an error toast while editing stays open

## Out of scope

- Crop and rotate (#34), which will be more adjustments of the same edit.
- Editing videos, GIFs and AVIF photos, raw files, and in the browser version.
- Local adjustments (brushes, masks, gradients), presets, copying an edit to other photos, batch editing.
- Reading or writing other tools' edit settings (Lightroom's `crs:` tags, darktable's history); see the plan.
- Managing backups of originals whose edited file was moved or deleted with other tools.

## Assumptions

- The libraries the user edits in are writable by them; the app needs no elevated rights.
- exiftool is available (as for indexing) to copy metadata into saved files.
- Adjustments are global (the same for every pixel apart from vignette and grain), so the preview of a zoomed-out photo matches the saved file.

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-021-01 |      | no test |
| REQ-021-02 |      | no test |
| REQ-021-03 |      | no test |
| REQ-021-04 |      | no test |
| REQ-021-05 |      | no test |
| REQ-021-06 |      | no test |
| REQ-021-07 |      | no test |
| REQ-021-08 |      | no test |
| REQ-021-09 |      | no test |
| REQ-021-10 |      | no test |
| REQ-021-11 |      | no test |
| REQ-021-12 |      | no test |
| REQ-021-13 |      | no test |
| REQ-021-14 |      | no test |
| REQ-021-15 |      | no test |
| REQ-021-16 |      | no test |
