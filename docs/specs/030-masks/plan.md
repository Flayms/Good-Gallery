# 030 · Masks · Plan

## Approach

- **`domain::edit`:** `Recipe::masks: Vec<LocalMask>` (a `MaskShape`, `invert`, and a recipe of light and color, as a part's). `Part::Mask(i)` lets the panel, undo and sliders treat a mask as they treat the subject. `Params::masks` holds each mask's numbers; `Params::pixel_glowing` blends them after the parts.
- **Shape parameters** are numbers in the recipe, so they cost no texture and don't depend on the photo's size. The gradient is in the original's coordinates, as the crop is (spec 029).
- **`edit.wgsl`:** the same formulas over a fixed `array<LocalMask, 8>`, with a count. `flags >> 8` names the mask to tint (index + 1), for the shader and `cpu_preview`.
- **`app::edit`:** a _Masks_ section last in the panel, with one button per kind; below the part switch one button per mask, once there are any; a chosen mask's controls above _Light_ and _Color_. A chosen gradient shows the whole photo, with its two handles (`gradient_input`, `paint_gradient`), as the crop does.

## Tasks

- [x] `domain::edit`: shapes, weights, `Part::Mask`, recipe round trip and sanitizing (REQ-030-02 … REQ-030-06, REQ-030-08)
- [x] Shader and CPU preview, parity test (REQ-030-07, REQ-030-08)
- [x] Saving (REQ-030-08)
- [x] Panel and handles (REQ-030-01, REQ-030-03, REQ-030-06, REQ-030-07)
- [x] Shapes combined in a mask; the subject and background among them (REQ-030-10, REQ-030-11)
- [ ] Phase after that: brush and freeform

## Decisions

### 2026-11-02 · A mask keeps its first shape and adds `more`

**Decided:** `LocalMask::shape` stays the first shape, and `more` holds up to three `MaskPart`s (a shape and how it combines), so masks saved before this read as they are. Add is the larger of the two values, subtract the smaller of the value and the inverse of the shape, intersect the smaller, as add, subtract and intersect work in Lightroom. Invert applies to the whole result. A further shape defaults to _Intersect_, the usual reason to add one to a gradient.
**Rejected:** a list of parts in place of `shape` (saved edits would need migrating), multiplying instead of the smaller value (two soft shapes would darken each other's selection), a separate Part::Subject migration (the switch does its job and saved edits keep rendering the same).

### 2026-11-02 · Four shapes per mask

**Decided:** four shapes in a mask, as fixed slots in the shader's uniform. FXC inlines every call, so this is what eight masks cost at compile time: watch how long the editor's pipeline takes on Windows if the numbers ever grow.
**Rejected:** a loop over a count (FXC can't compile it, see `docs/known-pitfalls.md`).

### 2026-11-02 · Masks as a list beside the parts

**Decided:** an ordered list of at most 8 masks, each with one shape, applied top to bottom after the subject and background. The subject and background stay as they are for now, so saved recipes render as before, and the issue's components (a mask made of several shapes, with add, subtract and intersect) can come later by giving a mask more shapes.
**Rejected:** components in this first phase (much more panel for something not yet needed to learn how masks get used), replacing the parts now (a migration with nothing to gain yet).

### 2026-11-02 · Ranges read the photo before developing

**Decided:** luminance and color masks read the stored pixel, after a blur, not the developed one. A selection stays the same while the user moves sliders, in any order of masks. Luminance is on the encoded scale, as the eye and the sliders' numbers see it.
**Rejected:** reading the developed pixel (a selection would shift as its own adjustments change it).

### 2026-11-02 · Buttons, not a menu, for adding masks

**Decided:** three buttons in a _Masks_ section at the panel's end, closed at first, so the panel stays as it was for those who don't use masks.
**Rejected:** the buttons at the panel's top (they push every section down), one _Add mask_ menu (hides the choice behind one more click, for three items).
