# 021 · Photo editing · Plan

## Approach

### Decisions

The issue left these open; approved as proposed:

1. **An edit is a recipe, the file is its rendering.** An edit is a small set of values (`domain::edit::Recipe`: light, curves, color, effects, later crop and rotation for #34) with a version number. Saving renders the recipe from the **unedited original** into the target file. Editing again starts from the recipe and the original, never from saved pixels, so a JPEG is re-encoded once no matter how often it is edited (REQ-021-11).
2. **Reversibility: a backup beside the library, never in the user profile.** Before a file is replaced for the first time, the original is moved to `<library>/.good-gallery/originals/<relative path>` (hidden on Windows). Dot folders are already housekeeping folders, so backups are never indexed. Later saves keep that first backup; _Revert_ moves it back. The backup travels with the library (NAS, other computers, a rebuilt index) and doesn't fill the local disk.
   _Rejected:_ backups in `%APPDATA%` (lost with the profile, unknown on other computers, fill a small system disk), keeping no backup (not reversible), versions beside the file (`IMG_1_original.jpg` would be indexed and clutter folders).
3. **Storage: in the saved file, mirrored in the index. No sidecar.** The recipe is written into the saved file's XMP in our own namespace (`gg:EditRecipe` as JSON, `gg:EditVersion`, `gg:EditSource` = relative path of the unedited original: the backup for _Save_, the source file for _Save as copy_). Indexing reads it like any other tag into a new `media_edits` table, so the viewer knows an item is edited and where its original is. This also decides #34.
   _Why not the index only:_ removing and re-adding a library, or another computer, would lose every edit's link to its original. _Why not a sidecar:_ Lightroom, darktable and digiKam already write `.xmp` sidecars with both naming schemes (REQ-004-10), and merging into theirs risks breaking them. The file has to be written anyway.
4. **No `crs:` or darktable compatibility.** Adobe's adjustments are proprietary and render differently from ours, and a file whose pixels already contain an edit and that also carries `crs:` values would get the edit applied twice by Lightroom. Importing `crs:` values as a starting point could come later and would be its own spec.
5. **GPU for the preview, CPU for saving, one set of formulas.** The preview is a fragment shader (`edit.wgsl`) that draws the viewer's original texture through the recipe, so sliders answer within a frame at any zoom. Saving runs the same formulas in Rust (`domain::edit::render`, f32 per pixel, rayon over rows) at full resolution, because the viewer's texture may be shrunk to the GPU's limit and readback would cap the size. A test renders test patterns with both and compares them within 1/255 (REQ-021-02, REQ-021-13).
6. **Color and bit depth (the mistakes to avoid).** Decode to 32-bit float, never to 8 bits in between; linearize with the sRGB transfer curve and do the maths on linear light (exposure, white balance, vignette are multiplications there); tone and curve operations on a perceptual (sRGB-encoded) value of luminance, as editors do, so curves behave as users expect. Work in the file's own RGB primaries and keep its ICC profile in the output (no silent conversion to sRGB, no stripped profile). Encode with triangular dithering to 8 bits, or keep 16 bits for 16-bit PNGs. Formats stay: JPEG (quality 95, the original's chroma subsampling), PNG (same bit depth, lossless), WebP (lossless if the original was, else quality 95).
7. **Date modified:** a setting _When saving edits_ under a new _Editing_ section of Settings, stored as `editingMtime` (`"keep"` default, `"update"`). _Keep_ is the default so photos without a date taken keep their place in the gallery; the setting's description notes that some sync tools look only at the date modified.

### Formulas (`domain::edit`)

Per pixel, in this order, each step the identity at its neutral value:

1. White balance: multiply linear RGB by a Bradford-adapted gain from temperature / tint (relative, −100…+100 maps to about ±3000 K around D65, as editors do for JPEGs); the picker solves for the gains that make the picked 5 × 5 average neutral.
2. Exposure: `rgb × 2^EV`.
3. Tone: on perceptual luminance `L`, a piecewise smooth tone curve built from blacks / shadows / highlights / whites (smoothstep masks over fixed tone ranges) and contrast (an S-curve around 0.5); the RGB ratio is kept, so tone doesn't shift hue.
4. Curves: monotone cubic (Fritsch–Carlson) through the points, master then per channel, baked into 4096-entry tables (a 1D texture on the GPU).
5. Saturation and vibrance in a perceptual space (OkLCh chroma scaling); vibrance weights by `1 − chroma` and dampens hues in the skin range.
6. Vignette: multiply by a smooth radial falloff of the photo's own aspect, from the midpoint outward.
7. Grain: value noise from a hash of the pixel's position relative to the photo's size (not the screen), so preview and saved file match and a save repeats exactly (REQ-021-06).

### Saving (`media::edit`)

- **Copy:** render → encode to `<name> (edited).tmp` in the same folder → exiftool copies all metadata from the original (`-tagsFromFile <original> -all:all -icc_profile -Orientation=1`, with a config file for the `gg` namespace) and writes the recipe → rename to the free name. The sidecar is copied under the new name.
- **Over the original:** if there is no backup yet, move the original into the backup folder (create with `create_dir_all`, check size afterwards), then render from the backup into a temp file in the file's folder, copy metadata as above, rename over the file. A failure before the rename leaves the original or the backup in place, and a backup without a replaced file is moved back (REQ-021-16).
- **Revert:** rename the backup over the file (same volume: one rename; across volumes: copy, fsync, rename), remove empty backup folders.
- **Afterwards:** set the date modified per the setting, then tell the indexer to re-read that one item (a changes scan of the path) and drop its thumbnails, since with _Keep_ the thumbnail key (size, mtime) might not change. The watcher's own report of the write is merged into the same changes scan.
- All paths go through `domain::media::is_safe_rel_path` (REQ-011-08); exiftool gets them as in REQ-011-09.

### UI

- `viewer::Mode::Edit`: the control bar gains _Compare_, _Undo_ / _Redo_, _Reset all_, _Cancel_ and a split _Save_ / _Save as copy_ button. The adjustments panel is a glass panel on the right in the info panel's place, with collapsible sections _Light_, _Curves_, _Color_, _Effects_, sliders with a numeric field, and a 256 × 256 curve editor with channel tabs. Undo history is a `Vec<Recipe>` with a cursor, one entry per finished drag.
- The edited photo is drawn with an `egui_wgpu` paint callback (like the glass pass) instead of the plain texture: same texture, the recipe as a uniform buffer, the curve tables as a 1D texture.
- The info panel shows _Edited_ with _Revert to original_ for items saved over their original.
- `design.md`'s subject gets a sentence: light edits in the viewer, still no library management or editing workflow.

### Changes to other specs

- **REQ-004-01** (read-only) becomes: no media file or sidecar is changed _except when the user saves or reverts an edit (spec 021)_. Glossary: _Read-only_ updated; new terms _Edit_, _Recipe_, _Backup of the original_.
- **REQ-010-01** gets the `editingMtime` setting with its default.
- **REQ-004** metadata extraction reads the `gg:` tags; no `METADATA_VERSION` bump, since no existing file has them.
- **Architecture:** a decision entry for 1–6.

### Delivery

Split into four PRs, each green and usable on its own:

1. Recipe and formulas in `domain`, CPU render, GPU shader, parity tests; edit mode with _Light_ and _Color_, compare, reset, undo; nothing can be saved yet.
2. Saving: copy, over the original with backup, revert, metadata, date modified setting, re-indexing; spec changes to 004 and 010.
3. Curves and effects.
4. (#34) Crop and rotate on top of the same recipe.

### Deviations

- **White balance:** gains on linear RGB along a blue–yellow and a green–magenta axis, ±1 stop at ±100, normalized to keep white's luminance, instead of a Bradford adaptation around D65. It is exact for the picker (closed form) and the same on CPU and GPU; ±0.5 stops was too little to neutralize ordinary casts.
- **JPEG** is encoded by the `image` crate at quality 95 with its own chroma subsampling (it can't take the original's).
- **The color profile** is copied by exiftool (`-icc_profile`) for every format, not written by the encoders.
- **Sliders** show their value as text; there is no numeric input field. A double-click resets one.
- **Not rendered on the GPU for saving**, and no rayon: `media::edit::render` splits rows over `std::thread::scope`.
- **The preview without a wgpu renderer** (kittest) is rendered on the CPU from a copy of at most 512 px.
- **Grain** follows the photo's full size from the index; in the zoomed-out preview it is point-sampled, so it looks a little coarser than in the saved file viewed at the same size.
- **Esc** while editing stops editing (asking first with unsaved changes); it no longer closes the viewer at once.
- **Controls:** _Undo_, _Redo_ and _Compare_ sit in the panel's header, _Reset all_, _Cancel_, _Save as copy_ and _Save_ in its footer; the control bar only gains _Edit_ (a toggle). There is no `viewer::Mode`: the viewer holds an `Option<edit::EditSession>`.
- **GPU and CPU agree within two levels** of 255, not one (different `pow` implementations; measured on lavapipe).
- **Brightness** (#70, REQ-021-17) is a gamma on the perceptual luminance between the black and white points, `L^(2^(−brightness/100))`, applied before shadows, highlights and contrast. It sits in _Light_ after _Exposure_; old recipes read it as 0.
- **The histogram** (#70, REQ-021-18) counts, in 128 bins, the values the curves receive (after white balance, exposure and tone), from about 25,000 pixels of the 512 px copy, recounted on the CPU when an adjustment before the curves changes. Bars are scaled to the tallest bin apart from the two ends, so clipped blacks or whites don't flatten the rest.
- **Delivered in one PR**, not four (the user asked for all of it at once). Crop and rotate (#34) remain.

## Tasks

- [x] 1. `domain::edit`: `Recipe` (serde, version), neutral checks, per-step formulas, curve tables; unit tests per step (REQ-021-03, REQ-021-05)
- [x] 2. CPU render in f32 with dithering; 16-bit and ICC round trip at neutral (REQ-021-13)
- [x] 3. `edit.wgsl` and the paint callback; GPU / CPU parity test on test patterns (REQ-021-02)
- [x] 4. Viewer edit mode: _Edit_ entry points and which items offer it, adjustments panel, live preview (REQ-021-01, REQ-021-02)
- [x] 5. Compare, reset, undo / redo, leave-without-saving dialog; snapshots (REQ-021-07, REQ-021-08)
- [x] 6. Migration `media_edits`; `gg:` namespace config for exiftool; extraction reads the recipe (REQ-021-11)
- [x] 7. `media::edit`: encode per format, metadata copy, save as copy, save over with backup, revert, failure paths (REQ-021-09, REQ-021-10, REQ-021-12, REQ-021-14, REQ-021-16)
- [x] 8. Setting _When saving edits_, date modified on save and revert (REQ-021-15)
- [x] 9. Re-index and thumbnail refresh after save; re-edit loads the recipe and renders from the original (REQ-021-10, REQ-021-11)
- [x] 10. Curves editor and curve formulas (REQ-021-04)
- [x] 11. Grain and vignette, deterministic grain test (REQ-021-06)
- [x] 12. REQ-004-01, REQ-010-01, glossary, design.md, architecture decision, README screenshot of the editor
- [x] 13. (#70) Brightness slider on CPU and GPU (REQ-021-17)
- [x] 14. (#70) Histogram behind the curve editor (REQ-021-18)
