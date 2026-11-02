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
6. **The viewer asks the session for the size to fit.** `natural` in `viewer.rs` becomes the session's output size while editing, except in _Crop_ (the whole turned photo) and _Compare_ (the whole unedited photo). One `Geometry` type in `domain::edit` maps screen to original and back, for the white balance picker, the blur's focus handle and the glass backdrop that records the photo.
7. **Face regions in the file are rewritten on save.** `metadata_args` stops copying `RegionInfo` and `RegionInfoMP` as they are. Saving reads them, moves them through `Geometry` and writes MWG `RegionInfo`, keeping non-face regions. `RegionInfoMP` is converted to it, so a stale copy can't disagree.
8. **Face regions in a sidecar are moved when read.** `meta::rules::extract_metadata` knows which source a region came from and the saved recipe, so a sidecar's regions go through the same `Geometry`. We don't rewrite other tools' sidecars (021's decision 3).
   _Rejected:_ dropping sidecar regions when a crop is saved (the faces are lost in the viewer).

### Risks

- `glass` records the photo as a backdrop by texture coordinates, with no turn. A turned photo needs a turned quad there, or the chrome blurs the wrong part.
- The tile's shape changes on save, which earlier edits never did, so the masonry and date groups must take the new dimensions from re-indexing without a stale frame.

## Tasks

Three PRs, each green and usable on its own:

1. Domain and saving: nothing to see in the UI yet.
   - [ ] `Geometry`, recipe fields, tidy and JSON round trip (REQ-029-07)
   - [ ] CPU render: crop, turn, vignette on the crop (REQ-029-04, REQ-029-06)
   - [ ] Face regions: rewrite on save with exiftool, move sidecar regions when read (REQ-029-08)
   - [ ] Amend REQ-021-14 and 021's Out of scope
2. Rotation in the UI.
   - [ ] Shader mapping and GPU / CPU parity with geometry (REQ-029-01, REQ-029-04)
   - [ ] Viewer size, picker, blur focus and glass backdrop through `Geometry` (REQ-029-01, REQ-029-04)
   - [ ] _Rotate left_, _Rotate right_; undo, reset, compare (REQ-029-01, REQ-029-05)
   - [ ] Gallery tile takes the new shape (REQ-029-06)
3. Crop in the UI.
   - [ ] Pure drag logic in `domain`: edges, corners, moving, ratio, minimum size (REQ-029-02, REQ-029-03)
   - [ ] _Crop_ with its frame, darkening, grid and ratio picker (REQ-029-02, REQ-029-03, REQ-029-05)
   - [ ] Screenshots of the running app, `README.md`, spec status

## Decisions

The decisions above move here as they are carried out.
