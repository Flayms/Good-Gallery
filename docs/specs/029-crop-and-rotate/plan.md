# 029 · Crop and rotate · Plan

## Approach

### Decisions

The issue (#34) left these open; to review in the PR:

1. **A spec of its own**, as 022, 023 and 028 are, on spec 021's recipe, save and revert. No new storage: this is what 021's plan reserved.
2. **The recipe gets `quarter_turns` (0–3) and `crop: Option<[f32; 4]>`.** The crop is `x, y, w, h` as fractions of the upright, unturned original, so it stays on the content: turning the photo changes no crop values. Older recipes have neither (`RECIPE_VERSION` stays 1, as in 022).
   _Rejected:_ the crop in the turned frame (every turn would have to rewrite it, and rounding would add up).
3. **Geometry last.** Light, color, blur, the subject mask and grain keep working in the original's coordinates, on the whole photo, then the result is cropped and turned. So the mask needs no change, and pixels just outside the crop still feed the blur. Only the vignette takes the crop's center and size.
   _Rejected:_ cropping first (faster to save, but the blur would darken at the crop's edge and masks would need new coordinates).
4. **No orientation tag.** One path through the recipe. Saving already bakes orientation into the pixels and writes tag 1 (`media::edit::metadata_args`).
5. **Preview: same shader, new mapping.** `edit.wgsl` computes `uv` from the rect's `t`. It gets a turn and the crop in that mapping, and the vignette gets the crop. GPU and CPU stay compared by `req_021_02_req_021_06_the_gpu_preview_matches_the_cpu_render`, extended with geometry.
6. **The viewer asks the session for the size to fit.** `natural` in `viewer.rs` becomes the session's output size while editing, except in _Crop_ (the whole turned photo) and _Compare_ (the whole unedited photo). One `Geometry` type in `domain::edit` maps output to original and back, for the white balance picker and the blur's focus. A saved edit's item only knows the saved file's size, so the session works out the whole photo's from the recipe.
7. **Face regions in the file are rewritten on save.** `metadata_args` stops copying `RegionInfo` and `RegionInfoMP` as they are. Saving reads them, moves them through `Geometry` and writes MWG `RegionInfo`, keeping non-face regions. `RegionInfoMP` is converted to it, so a stale copy can't disagree.
   Regions are also moved when saving only bakes an orientation into the pixels, which left them in the wrong place before (REQ-021-14). They are written through exiftool's `-json=` from a temporary file, so names need no escaping. A person who loses every region is added to `PersonInImage`, so their People tag stays.
8. **Face regions in a sidecar are moved when read.** `meta::rules::extract_metadata` knows which source a region came from and the saved recipe, so a sidecar's regions go through the same `Geometry`. We don't rewrite other tools' sidecars (021's decision 3).
   _Rejected:_ dropping sidecar regions when a crop is saved (the faces are lost in the viewer).

### Risks

- ~~`glass` records the photo as a backdrop with no turn.~~ It doesn't while editing: the preview records nothing, so the turned photo never reaches it.
- The tile's shape changes on save, which earlier edits never did. Checked in the running app: re-indexing brings the new size, and the tile takes it.

## Tasks

Three PRs, each green and usable on its own:

1. Domain and saving: nothing to see in the UI yet.
   - [x] `Geometry`, recipe fields, tidy and JSON round trip (REQ-029-07)
   - [x] CPU render: crop, turn, vignette on the crop (REQ-029-04, REQ-029-06)
   - [x] Face regions: rewrite on save with exiftool, move sidecar regions when read (REQ-029-08)
   - [x] Amend REQ-021-14 and 021's Out of scope
2. Rotation in the UI.
   - [x] Shader mapping and GPU / CPU parity with geometry (REQ-029-01, REQ-029-04)
   - [x] Viewer size, picker and blur focus through `Geometry` (REQ-029-01, REQ-029-04)
   - [x] _Rotate left_, _Rotate right_; undo, reset, compare (REQ-029-01, REQ-029-05)
   - [x] Gallery tile takes the new shape (REQ-029-06)
3. Crop in the UI.
   - [x] Pure drag logic in `domain`: edges, corners, moving, ratio, minimum size (REQ-029-02, REQ-029-03)
   - [x] _Crop_ with its frame, darkening, grid and ratio picker (REQ-029-02, REQ-029-03, REQ-029-05)
   - [x] Screenshots of the running app, `README.md`, spec status

## Decisions

All carried out as listed under _Approach_, with the changes noted there. Added while building: _Swap_ turns the frame around its center rather than fitting inside the current frame, which would shrink it a little more on every press; a person's People tag stays through `PersonInImage` when their last region is cropped away.
