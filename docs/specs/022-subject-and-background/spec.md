# 022 · Subject and background

**Status:** Implemented

## Goal

A portrait with a dull sky behind it, or a product shot against a busy wall, needs the subject and the background adjusted differently. Instead of painting masks, users who edit a photo (spec 021) choose _Subject_ or _Background_, and the app finds the photo's main subject itself, the way Snapseed does. Each part gets its own light and color, on top of the edit of the whole photo, with a soft transition at the subject's edge.

## Requirements

### REQ-022-01 · Choose the part to edit

- **Given** editing a photo
- **When** the user chooses _Photo_, _Subject_ or _Background_ at the top of the adjustments panel
- **Then** the _Light_ and _Color_ sliders shown change that part; _Curves_ and _Effects_ are only offered for the whole photo
- **And** _Photo_ is chosen when editing starts

### REQ-022-02 · Find the subject

- **Given** editing a photo
- **When** the user first chooses _Subject_ or _Background_, or edits a photo that was saved with subject or background adjustments
- **Then** the app finds the photo's main subject on this computer, without network access, while the panel shows that it is looking
- **And** if no subject can be found, the panel says so, and the whole photo can still be edited

### REQ-022-03 · Separate adjustments

- **Given** the subject was found
- **When** the user changes a _Light_ or _Color_ adjustment of _Subject_ or _Background_
- **Then** only that part changes, on top of the adjustments of the whole photo, fading softly across the subject's edge
- **And** each at 0 leaves the photo unchanged; _Compare_, _Undo_, _Redo_ and _Reset all_ include them (REQ-021-07)

### REQ-022-04 · Show the part

- **Given** _Subject_ or _Background_ chosen and the subject found
- **When** the user turns on _Show selection_
- **Then** the chosen part is tinted in the preview until it is turned off or another part is chosen
- **And** the tint is never saved

### REQ-022-05 · Saved and editable again

- **Given** an edit with subject or background adjustments
- **When** it is saved (as a copy or over the original) and edited again later
- **Then** the saved file looks like the preview, and editing again starts with the same adjustments for each part, applied to the subject as found again in the unedited original

## Out of scope

- Correcting the found subject by hand (brush, eraser), several subjects separately, or choosing which object is the subject.
- Curves, grain and vignette per part.
- Hair-fine edges: the transition is soft (see the plan).
- The browser version, which doesn't edit (spec 021).

## Assumptions

- "The main subject" is what stands out: the object a viewer looks at first (salient object detection). For most portraits, pets, products and landmarks that is what users mean.
- Finding the subject takes up to a few seconds on an ordinary CPU; no GPU is needed.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
