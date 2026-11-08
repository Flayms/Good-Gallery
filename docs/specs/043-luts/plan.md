# 043 · LUTs · Plan

## Approach

- **`domain::lut`** (pure): reads `.cube` text (`TITLE`, `LUT_3D_SIZE`, `DOMAIN_MIN` / `DOMAIN_MAX`, red fastest), samples trilinearly, and names a file by the first 16 hex digits of its SHA-1. `LutChoice { id, name, strength }` is the recipe's new `lut` field.
- **Pipeline:** `Params::new` takes the strength; `Params::with_lut` gives the table, which the caller has or not. `pixel_glowing` passes the stored color through the LUT first (`through_lut`), so the adjustments, the parts, the masks and a film look work on its result; the histogram sees it too. The shader mirrors it with a 3D `Rgba16Float` texture at binding 6 and the strength, size and domain at the end of the uniform; the hardware's linear filter interpolates between texel centers.
- **Store (`app::luts`):** `Luts::open(data/luts)` reads `<id>.cube` (kept byte for byte, so its id stays) and `<id>.name` for a file without a title. `import` parses, keeps and lists. One `Arc<Luts>` is shared by the app state (import on a thread through `FolderPicker::pick_lut_file`), the viewer and every edit session.
- **Saving:** `SaveEdit` / `SaveRequest` carry the table, which `render_file` passes to `Params`.

## Tasks

- [x] `domain::lut`: parse, sample, id, `LutChoice` (REQ-043-01, -02, -03)
- [x] Recipe field and the CPU pipeline, histogram (REQ-043-02, -03)
- [x] Shader and texture (REQ-043-02)
- [x] Store, import, the editor's LUT row and strength (REQ-043-01, -02)
- [x] Saving with the table (REQ-043-02, -03)

## Decisions

### 2026-11-08 · The LUT first

**Decided:** the LUT applies to the photo's own colors before every adjustment, as a camera profile does in other editors, so exposure and white balance fine-tune its result.
**Rejected:** last, after the adjustments, which makes every slider change what the LUT receives and its look with it.

### 2026-11-08 · Kept by content, not by path

**Decided:** an imported LUT is copied into the data folder and named by its content's hash, which the edit records. Moving or renaming the original file changes nothing, and an edit never renders with another table under the same name.
**Rejected:** remembering the file's path, which breaks when it moves. Putting the table into the photo's metadata, which takes 100 KB or more per photo at 33 points a side.

### 2026-11-08 · No bundled LUTs

**Decided:** none come with the app, as refined in the session: free LUT packs rarely state a license that allows bundling. The film looks (spec 028) stay the built-in looks.
