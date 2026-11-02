# 030 · Masks · Plan

## Approach

- **`domain::edit`:** `Recipe::masks: Vec<LocalMask>` (one `MaskShape`, `invert`, and a recipe of light and color, as a part's). `Part::Mask(i)` lets the panel, undo and sliders treat a mask as they treat the subject. `Params::masks` holds each mask's numbers; `Params::pixel_glowing` blends them after the parts.
- **Shape parameters** are numbers in the recipe, so they cost no texture and don't depend on the photo's size. The gradient is in the original's coordinates, as the crop is (spec 029).
- **`edit.wgsl`:** the same formulas over a fixed `array<LocalMask, 8>`, with a count. `flags >> 8` names the mask to tint (index + 1), for the shader and `cpu_preview`.
- **`app::edit`:** at the panel's top one list of the parts and the masks, then one button per kind of mask, then a chosen mask's controls, above everything else. `show_selection` is on when editing starts and stays as the user sets it; `adjusting` hides the tint while an adjustment's slider is held. A chosen gradient shows the whole photo, with its two handles (`gradient_input`, `paint_gradient`), as the crop does.

## Tasks

- [x] `domain::edit`: shapes, weights, `Part::Mask`, recipe round trip and sanitizing (REQ-030-02 … REQ-030-06, REQ-030-08)
- [x] Shader and CPU preview, parity test (REQ-030-07, REQ-030-08)
- [x] Saving (REQ-030-08)
- [x] Panel and handles (REQ-030-01, REQ-030-03, REQ-030-06, REQ-030-07)
- [x] Shapes combined in a mask; the subject and background among them (REQ-030-10, REQ-030-11)
- [x] What to edit first, one shape per mask, the tint on by default (REQ-030-07, REQ-030-15, REQ-030-16, #152)
- [x] Brush and freeform: `domain::paint`, a texture array in the shader, the panel and drawing on the photo (REQ-030-12 … REQ-030-14)

## Decisions

### 2026-11-02 · One list, one shape, and the tint on (#152)

**Decided:** the parts and the masks are one list at the panel's top, with the buttons to add a mask right under it, never folded away. A mask has one shape, and combining them is gone with the subject and background as shapes. The tint is on by default for all but _Photo_, whose tint would cover everything and say nothing, and it hides while an adjustment's slider is held, or a tinted photo couldn't be judged. Combining shapes wasn't in any release, so a saved edit with further shapes just loses them when it is read (`more` is an unknown field, a `subject` shape an unknown kind, REQ-030-08).
**Rejected:** a switch per mask to leave the subject out (not needed until someone misses _the sky, but not the person_), a one-time migration for files saved with development builds, and a tint that stays while adjusting (the issue's first wording).

### 2026-11-02 · Painted masks as points, drawn into a texture

**Decided:** strokes and outlines are points in the recipe (`Recipe::paintings`, four decimals, thinned when the user lets go), and the shapes name a painting by its place, which keeps `MaskShape` a plain copyable value. They are drawn into a grid of at most 1024 px on the longer side: the same `domain::paint` code for the preview's texture array, kept up to date per added segment while painting, and for saving, where `Params::with_paintings` hands the grids over (`Params::new` runs every frame, so it can't draw them). Painting is `max` and erasing `min`, so it gives the same result however many segments are drawn at a time. Mirrors the subject's mask: one layer per painting, bilinear on both sides.
**Rejected:** storing a bitmap (a compressed one would still be far above what JPEG's XMP segment holds), a mask image beside the file (the app keeps all of an edit in the file, spec 021), a limit per stroke (it's the total that has to fit).

### 2026-11-02 · 1,500 points

**Decided:** at four decimals a point takes about 16 bytes of JSON, so 1,500 points are about 24 KB, well inside the roughly 64 KB of a JPEG's XMP segment with room for the rest of the recipe and the file's own XMP. A test saves a full painting through exiftool and reads it back.
**Rejected:** relying on extended XMP (not every program reads it).

### 2026-11-02 · A mask keeps its first shape and adds `more`

Superseded by _One list, one shape, and the tint on_.

**Decided:** `LocalMask::shape` stays the first shape, and `more` holds up to three `MaskPart`s (a shape and how it combines), so masks saved before this read as they are. Add is the larger of the two values, subtract the smaller of the value and the inverse of the shape, intersect the smaller, as add, subtract and intersect work in Lightroom. Invert applies to the whole result. A further shape defaults to _Intersect_, the usual reason to add one to a gradient.
**Rejected:** a list of parts in place of `shape` (saved edits would need migrating), multiplying instead of the smaller value (two soft shapes would darken each other's selection), a separate Part::Subject migration (the switch does its job and saved edits keep rendering the same).

### 2026-11-02 · Four shapes per mask

Superseded by _One list, one shape, and the tint on_; the real loop over the masks stays, for the reason below.

**Decided:** four shapes in a mask, as fixed slots in the shader's uniform, with real loops over the masks and the shapes, so the shader holds one copy of a mask's code however many there are.
**Rejected:** one explicit call per mask and shape, as phase 1 had it to get past FXC's X3511: FXC inlines them all, which made the app's start on Windows seconds slower and rendered wrongly with 32 copies. The real cause was a variable-index write in `core` (`docs/known-pitfalls.md`).

### 2026-11-02 · Masks as a list beside the parts

**Decided:** an ordered list of at most 8 masks, each with one shape, applied top to bottom after the subject and background. The subject and background stay as they are for now, so saved recipes render as before, and the issue's components (a mask made of several shapes, with add, subtract and intersect) can come later by giving a mask more shapes.
**Rejected:** components in this first phase (much more panel for something not yet needed to learn how masks get used), replacing the parts now (a migration with nothing to gain yet).

### 2026-11-02 · Ranges read the photo before developing

**Decided:** luminance and color masks read the stored pixel, after a blur, not the developed one. A selection stays the same while the user moves sliders, in any order of masks. Luminance is on the encoded scale, as the eye and the sliders' numbers see it.
**Rejected:** reading the developed pixel (a selection would shift as its own adjustments change it).

### 2026-11-02 · Buttons, not a menu, for adding masks

The buttons stay; their place at the panel's end is superseded by _One list, one shape, and the tint on_.

**Decided:** three buttons in a _Masks_ section at the panel's end, closed at first, so the panel stays as it was for those who don't use masks.
**Rejected:** the buttons at the panel's top (they push every section down), one _Add mask_ menu (hides the choice behind one more click, for three items).
