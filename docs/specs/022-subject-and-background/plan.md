# 022 · Subject and background · Plan

## Approach

### Decisions

The issue left these open; decided as follows (to review in the PR):

1. **Model: U²-Net-p** (Qin et al. 2020, salient object detection, Apache-2.0), the 4.6 MB variant, 320 × 320 input, one soft mask out. It finds "what stands out", which matches Snapseed's subject better than person-only models.
   _Rejected:_ full U²-Net, ISNet, BiRefNet, BRIA RMBG (170 MB to 1 GB, better edges, but too large to ship and too slow on a CPU; RMBG is non-commercial too); MODNet and MediaPipe Selfie (people only).
2. **Runtime: `tract-onnx`, pure Rust on the CPU.** No native library to bundle per platform and nothing to download (REQ-011-11).
   _Rejected:_ `ort` (an ONNX Runtime library per platform, a fourth bundled helper), `candle` (the network would have to be rebuilt by hand).
3. **The model is part of the executable** (`include_bytes!` from `crates/media/models/u2netp.onnx`, with its license beside it). The executable grows by 4.6 MB; nothing can be missing at run time.
4. **The recipe gets `subject` and `background`**, each the _Light_ and _Color_ values of a recipe (`Option<Box<Recipe>>`, left out while neutral; `RECIPE_VERSION` stays 1, since older recipes simply have none). Per pixel: the whole photo's white balance, exposure, tone, curves and chroma; then the part's white balance, exposure, tone and chroma on that result; the two parts blended by the mask; then vignette and grain over the whole.
5. **The mask is not stored.** Editing again finds it again from the unedited original (REQ-022-05). For saving, the session hands its mask over with the recipe, so the file matches the preview exactly; without one (never from the UI) saving finds it itself.
6. **Edges:** the 320 × 320 mask is scaled to the photo bilinearly (the network's output is soft already). Hair and fine detail get a soft transition; an edge-aware refinement (guided filter) could come later behind the same `Mask`.

### Code

- `domain::edit`: `Part` (Photo, Subject, Background), `Recipe::part(_mut)`, `Adjustment::in_parts`, `Recipe::tidy` (drops neutral parts and anything but light and color from them), `Mask` (soft values on a grid, `sample(uv)` bilinear), `Params::pixel_masked(rgb, uv, size, subject)`.
- `media::subject::find(&RgbaImage) -> Result<Mask, String>`: resize to 320 × 320, normalize with ImageNet's mean and deviation, run, scale the output to 0–1 (min–max, as rembg does). The model is optimized once per process (`OnceLock`).
- `media::edit::render` takes an optional mask; `SaveRequest` gets `subject`.
- `app::edit`: the part switch above the sections, `Show selection`, finding the mask on a thread, the mask as a second texture (`R8Unorm`, a 1 × 1 one while there is none) and the parts' numbers in the uniform; `edit.wgsl` gets the same blending.

## Tasks

- [x] 1. Spec and plan (this)
- [x] 2. `domain::edit`: parts in the recipe, tidy, JSON round trip, `Mask`, masked pixel formula (REQ-022-03, REQ-022-05)
- [x] 3. `media::subject` with tract and the embedded model (REQ-022-02)
- [x] 4. Saving with the mask (REQ-022-05)
- [x] 5. Shader: mask texture, parts, selection tint; GPU / CPU parity with a mask (REQ-022-03, REQ-022-04)
- [x] 6. Panel: part switch, finding, Show selection, panel states; snapshots (REQ-022-01, REQ-022-02, REQ-022-04)
- [x] 7. Glossary, architecture decision, README, third-party license
