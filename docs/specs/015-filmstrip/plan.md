# 015 · Filmstrip · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Strip** (`domain::viewer::Strip`): one row of square thumbnails of the whole gallery below the media; only the thumbnails in view are drawn, and the gallery's texture streamer is asked for them ahead of the tiles. The shown item is outlined and centered as far as the ends allow, again when the strip's width changes (REQ-015-03).
- **Toggle** (REQ-015-01, -02): `f` and the _Filmstrip_ button, remembered as `viewer.filmstrip` in the `settings` table; hidden until shown the first time.
- **Browse** (REQ-015-04, -05): a click or Enter jumps; the vertical wheel scrolls sideways, touchpad swipes keep their direction. Videos get a play symbol. The strip fades with the viewer's controls (REQ-015-06); the media area fits above it.

## Differences

- The strip jumps to the shown item instead of scrolling smoothly.

## Tasks

- [x] Strip layout, centering, thumbnails in view (REQ-015-01 … -03, -05)
- [x] Toggle remembered, click and Enter jump, fades with the controls (REQ-015-02, -04, -06, -07)
- [x] Keyboard focus into the viewer, so Tab reaches the filmstrip (REQ-015-04): the viewer is egui's modal layer while it is open
