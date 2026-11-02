# 028 · Film looks · Plan

## Approach

A look is a recipe of the editor's own adjustments and curves (`domain::look`), so the preview shader, the CPU path and saving need no new stage: `Params::new` folds the chosen look into the parameters it already builds.

- `Recipe::look: Option<LookChoice>` (look, strength 0–100, grain 0–200), saved in `gg:EditRecipe`; a look only a newer version knows reads as none. Recipes without a look serialize as before.
- `Params::new` with a look: each adjustment is the user's value plus strength × the look's (clamped to its range); grain is the user's plus grain × the look's; vignette midpoint and grain size are the look's where the user has none. The curve table is the look's curves (blended towards straight by strength), then the user's.
- The panel gets a _Look_ section first (photo only): selectable buttons, then _Strength_ and _Grain_ through the slider the adjustments use.

## Tasks

- [x] `domain::look`: the six looks; `Recipe::look`, its range and parsing (REQ-028-01, REQ-028-04)
- [x] `Params::new` folds the look in: strength, grain, curves, adjustments on top (REQ-028-02 … REQ-028-04)
- [x] Panel: _Look_ section, _Strength_, _Grain_ (REQ-028-01 … REQ-028-03)
- [x] Saved file matches the preview (REQ-028-05)
- [x] Tune the looks by eye; screenshots in the pull request

## Decisions

### 2026-11-02 · Colors per hue with eight nodes

**Decided:** a look gives eight nodes around Oklab's color wheel (turn, chroma's factor, lightness), blended between neighbours and applied after the photo's chroma step, at the look's strength (`Look::hue_nodes`, `edit.wgsl`'s `turn_hues`). The user's own adjustments stay global. This replaces the earlier "no hue-selective adjustments for now".
**Rejected:** a 3D LUT per look, for the same reasons as below (a new 3D texture, files to make and license); a hue panel for the user, which nobody asked for here.

### 2026-11-02 · Grain like film's, only for looks

**Decided:** a look's grain follows the brightness (`0.35 + 0.65 · 4v(1−v)`) and, for color films, mixes in a noise per channel. `Params` carries both, and the shader gets them in `film`. _Grain_ without a look stays as it was, so edits without a look render as before.
**Rejected:** changing all grain (it would change saved edits that use no look, for a slider that REQ-021-06 calls plain noise).

### 2026-11-02 · Looks from the editor's own adjustments

**Decided:** a look is a `Recipe` of the existing adjustments and curves, blended in by `Params::new`. The shader, the CPU formulas and saving stay as they are, so the preview and the saved file can't drift apart, and REQ-021-13's test covers looks too.
**Rejected:** a 3D LUT per look (a new stage in `edit.wgsl` and the CPU path, a 3D texture, and LUT files to make and license), copying third-party LUT packs (licenses), hue-selective adjustments for now (the editor has none; looks approximate them with curves and vibrance).
