# 023 · Blur · Plan

## Approach

### Decisions

The issue left the kinds of blur open; decided as follows (to review in the PR):

1. **Three blurs.** _Behind subject_ (the portrait look, using the subject of spec 022), _Tilt-shift_ (a sharp horizontal band, the miniature look) and _Radial_ (one sharp spot). They cover what editors offer most (Snapseed's _Lens Blur_ is radial, phones' portrait mode is behind the subject). A plain soften of the whole photo was left out: it hardly improves a photo.
2. **One recipe, one formula.** `blur_kind`, `blur` (strength), `blur_size`, `blur_feather` and `blur_focus` join the recipe; `domain::edit::Params::blur_sigma` gives the blur at each spot as σ in pixels of the full photo, from 0 at the focus (or on the subject) to the strength (up to 1.5 % of the longer side) beyond the transition. Unknown kinds from newer versions read as none.
3. **A pyramid, not a convolution.** A per-pixel varying Gaussian is too slow live on large photos. The photo is halved by 2 × 2 boxes again and again (the GPU texture's own mip chain: `domain::edit::Pyramid` builds the same levels on the CPU), and each spot samples the level for its σ with a cubic B-spline, mixing the two levels around a fractional level (σ ≈ 0.65 · 2^level pixels). Smooth, independent of the strength's size, and the same on the GPU (four bilinear taps per level, `edit.wgsl`) and the CPU (`media::edit::render`, `app::edit::cpu_preview`).
4. **Blur first, then develop.** The photo is blurred as a lens would, then light, color and the parts apply to the blurred pixels; transparency stays sharp.

### Deviations

- The kind _Background_ is labelled _Behind subject_: the part switch above it already has a _Background_.
- The slider is _Strength_ under a _Blur_ heading, not _Blur_ twice.
- The blur is first in _Effects_, before grain and vignette.
- Behind the subject, the subject's colors bleed a little into the blurred background next to it (the pyramid doesn't know the mask).

## Tasks

- [x] 1. Spec and plan
- [x] 2. `domain::edit`: blur fields, `BlurKind`, `blur_sigma`, `Pyramid`, `blur_lod` (REQ-023-01 … REQ-023-04)
- [x] 3. Saving through the pyramid; a background blur needs the subject (REQ-023-04, REQ-022-05)
- [x] 4. Shader: blur before developing; GPU / CPU parity deep into the pyramid (REQ-023-04)
- [x] 5. Panel: kind, strength, focus size, transition, _Place focus_; snapshot (REQ-023-01 … REQ-023-03)
- [x] 6. Glossary, architecture, README
- [x] 7. Lens blur: weighted pyramid, disc of taps on CPU and GPU, _Soft_ / _Lens_, parity and snapshot (REQ-023-05, #202)

## Decisions

### 2026-10-31 · Blur from an image pyramid

**Decided:** blurs (spec 023) sample a pyramid of the photo, halved by 2 × 2 boxes as the GPU texture's mip chain, with a cubic B-spline at the level of each spot's blur (σ ≈ 0.65 · 2^level), mixing two levels in between. The shader uses the texture's own mips (four bilinear taps per level), saving builds the same levels in 32-bit float (`domain::edit::Pyramid`). The blur comes before the other adjustments, as a lens's would.
**Rejected:** a convolution per pixel with a varying kernel (too slow live on large photos), blurring on the GPU into extra textures per change (a readback or a second pipeline, and a different result when saving).

### 2026-11-06 · Lens blur from a weighted pyramid (#202)

**Decided:** _Lens_ is a choice beside _Soft_ for every kind of blur (a flag on the recipe), not a fourth kind: where the photo blurs and how it blurs are separate. Each pixel is weighted by its brightness (`domain::edit::lens_weight`, up to 41 × for highlights) before a second pyramid is built of the weighted values with the weight beside them; a disc of 48 taps on a golden-angle spiral (radius 2 σ) reads that pyramid trilinearly, one read per tap, at the level whose footprint matches the taps' spacing, and divides by the weights. The footprint and the weight were tuned by eye on a night scene: smaller footprints show the taps as grain inside the discs, larger ones soften their edges. Halving weighted values keeps a small light's share, so it spreads into an evenly lit disc instead of fading into grey. On the GPU the pyramid is a half-float texture built from the photo's texture in two small render passes (`lens.wgsl`), starting at the photo's level 1, which a disc never samples below, and cached per photo; saving and the CPU preview build it in 32-bit float (`domain::edit::blur_pixel`).
**Rejected:** weighting on the mips already there (the box filter has averaged a light away before its weight applies, so no discs); the editor's 512 px small copy as the source (same problem, and a different result from the saved file); taps at full resolution (gaps between taps miss small lights, and slow on large discs); a depth-dependent blur (needs a depth model; later, after #200's model work).
