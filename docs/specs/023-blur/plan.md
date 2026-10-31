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
