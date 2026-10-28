# 009 · Media viewer · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Pure logic** (`domain::viewer`): `ZoomView { scale, x, y }` (offset of the content's center from the viewport's center, scale 1 = fitted), `clamp_view`, `zoom_to` (keeps the point under the pointer), `fit_size` (never enlarged), `toggle_scale` (fitted ↔ actual pixels, at least 2×, at most 8×), `neighbours` (two places each way, nearest first), `controls_hidden`.
- **Viewer** (`app::viewer`): `AppState::viewer`, drawn in an `Order::Middle` area over the window while the gallery keeps drawing below (context menus and tooltips open above it). It takes its keys at the start of the frame, before anything else. It reads the shown item from the gallery's tile data by position and follows its media id across reloads ("Media not found" when it is gone, REQ-009-12). The gallery keeps the shown item in view, so closing lands on it.
- **Open, step, close** (REQ-009-01 … -03): click or Enter on a tile or row; ← / → and the buttons; Escape, _Close_, Alt + ←, the mouse's back button, or a click on the scrim (after egui's double-click window, so a double-click zooms instead).
- **Originals** (`media::original`, `app::originals`): two worker threads decode the wanted set (shown item, then neighbours nearest first, REQ-009-13), build the mips and create the `wgpu::Texture`. A request that leaves the set before it starts never runs, one that finishes after is dropped. Originals are shrunk only beyond the GPU's texture limit (8192 px). AVIF through ffmpeg. Failures say "Can't load the original. Showing a preview." (REQ-009-15).
- **Preview** (REQ-009-14): the item's thumbnail texture from the gallery's streamer (else the largest is requested ahead of the tiles; the ThumbHash only without one), at the original's place and size; the original fades in over it, or shows at once when preloaded.
- **Zoom and pan** (REQ-009-05, -16): wheel around the pointer (`exp(-delta · 0.002)`), `+` / `-` by 1.25×, `0` fits, click on the photo toggles fitted ↔ actual pixels, drag pans, 1×–8×, clamped.
- **Video** (REQ-009-06): the viewer talks to a `video::Playback`; the app's is `MpvPlayer` (libmpv), UI tests use `FakePlayback`. The frame is drawn fitted and rotated by `video-params/rotate`, with the preview until the first frame. Glass controls along the video's bottom: play / pause, time, seek slider (keyframe seeks while dragging, an exact one on release), mute, volume. Space plays / pauses, a click on the video too; ← / → seek 5 s and ↑ / ↓ change the volume while the last click was on the video or its controls, else they step. Play at the end starts over. Stepping and closing stop playback. Without libmpv, videos show their preview and say why.
- **Info panel and sheet** (REQ-009-08, -09, -11): `db::media::detail` (fields, absolute path, tags with their category) loaded on the gallery's loader thread; a 320 px panel in the viewer (`i`) and a sheet beside the gallery (context menu _Information_). A click on a tag closes the viewer and filters by it.
- **Show in Explorer** (REQ-009-10): `explorer.exe /select,"<path>"` on a job thread, started outside the app's job object (`CREATE_BREAKAWAY_FROM_JOB`, retried without it when a parent job forbids breakaway); a missing file gets an error toast.

## Known limits

- Originals are turned upright by their EXIF orientation as the `image` crate reads it; a photo oriented only in XMP would show sideways.
- Tab doesn't move focus into the viewer.
- While the window is minimized, display-synced video gets no presented frames (video waits, audio goes on).

## Tasks

- [x] Zoom math, preloading order, controls timing (REQ-009-05, -13, -16)
- [x] Originals: full-size upright decode, worker threads, failures (REQ-009-04, -13, -15)
- [x] Viewer: open, step, close, preview, missing items (REQ-009-01 … -03, -12, -14)
- [x] Info panel, sheet, filter by tag, Show in Explorer, context menus (REQ-009-08 … -11)
- [x] Video playback and controls (REQ-009-06)
- [ ] Automated test for the thumbnail preview (REQ-009-14; needs the GPU texture streamer)
- [ ] Pass exiftool's orientation to the original's decode, for photos oriented only in XMP
