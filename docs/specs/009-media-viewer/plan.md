# 009 · Media viewer · Plan

Records the approach that was taken. Lasting decisions are under _Decisions_ at the end.

## Approach

- **Pure logic** (`domain::viewer`): `ZoomView { scale, x, y }` (offset of the content's center from the viewport's center, scale 1 = fitted), `clamp_view`, `zoom_to` (keeps the point under the pointer), `fit_size` (never enlarged), `toggle_scale` (fitted ↔ actual pixels, at least 2×, at most 8×), `neighbours` (two places each way, nearest first), `controls_hidden`.
- **Viewer** (`app::viewer`): `AppState::viewer`, drawn in an `Order::Middle` area over the window while the gallery keeps drawing below (context menus and tooltips open above it). It takes its keys at the start of the frame, before anything else. It reads the shown item from the gallery's tile data by position and follows its media id across reloads ("Media not found" when it is gone, REQ-009-12). The gallery keeps the shown item in view, so closing lands on it.
- **Open, step, close** (REQ-009-01 … -03): click or Enter on a tile or row; ← / → and the buttons; Escape, _Close_, Alt + ←, the mouse's back button, or a click on the scrim (after egui's double-click window, so a double-click zooms instead).
- **Originals** (`media::original`, `app::originals`): two worker threads decode the wanted set (shown item, then neighbours nearest first, REQ-009-13), build the mips and create the `wgpu::Texture`. A request that leaves the set before it starts never runs, one that finishes after is dropped. Originals are shrunk only beyond the GPU's texture limit (8192 px). AVIF through ffmpeg. Failures say "Can't load the original. Showing a preview." (REQ-009-15).
- **Preview** (REQ-009-14): the item's thumbnail texture from the gallery's streamer (else the largest is requested ahead of the tiles; the ThumbHash only without one), at the original's place and size; the original fades in over it, or shows at once when preloaded.
- **Zoom and pan** (REQ-009-05, -16): wheel around the pointer (`exp(-delta · 0.002)`), `+` / `-` by 1.25×, `0` fits, click on the photo toggles fitted ↔ actual pixels, drag pans, 1×–8×, clamped.
- **Video** (REQ-009-06): the viewer talks to a `video::Playback`; the app's is `MpvPlayer` (libmpv), UI tests use `FakePlayback`. The frame is drawn fitted and rotated by `video-params/rotate`, with the preview until the first frame. Glass controls along the video's bottom: play / pause, time, seek slider (keyframe seeks while dragging, an exact one on release), mute, volume. Space plays / pauses, a click on the video too; ← / → seek 5 s and ↑ / ↓ change the volume while the last click was on the video or its controls, else they step. Play at the end starts over. Stepping and closing stop playback. Without libmpv, videos show their preview and say why. Frames are timed to the display (`video-sync=display-resample` + `override-display-fps`, one render per UI frame with `report_swap`, frames marked `REPEAT` skipped): timed to the audio clock, a 30 fps video on 60 Hz showed frames for 1 or 3 vblanks in up to 44 % of frames, varying run to run.
- **Info panel and sheet** (REQ-009-08, -09, -11): `db::media::detail` (fields, absolute path, tags with their category) loaded on the gallery's loader thread; a 320 px panel in the viewer (`i`) and a sheet beside the gallery (context menu _Information_). A click on a tag closes the viewer and filters by it.
- **Face regions** (REQ-009-17): the viewer keeps the shown photo's details loaded (`Gallery::detail`, kept while the viewer is open) and draws its regions over the photo rect, so they follow zoom and pan: a white box with a dark outline and the name in a dark pill below (above at the media area's bottom), the same in both appearances like tile badges. `p` and _Faces_ (only for photos with regions) toggle them; the choice is the setting `viewer.faces`. Hidden while the photo grows or shrinks and while editing.
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
- [x] Face regions over the photo (REQ-009-17)
- [ ] Automated test for the thumbnail preview (REQ-009-14; needs the GPU texture streamer)
- [ ] Pass exiftool's orientation to the original's decode, for photos oriented only in XMP

## Decisions

### 2026-10-26 · Video playback: libmpv, software render API, display-synced

**Decided:** video plays through libmpv (`libmpv-2.dll`, loaded at runtime with `libloading` through a small own FFI; `hwdec=auto-copy`). A render thread renders each frame with mpv's software render API (`rgb0`) at the size the video occupies on screen and writes it into a `wgpu::Texture`; the UI draws the texture as a quad, rotated by `video-params/rotate` (the software renderer ignores rotation). Timing is display-synced: `video-sync=display-resample` with the monitor's refresh rate; the UI wakes the render thread once per presented frame, which calls `report_swap` and skips frames mpv marks as repeats. The render thread never calls non-render libmpv functions. MP4 H.264, MOV HEVC (incl. 4K and rotation), MKV and WebM VP9 + Opus play with 0–4 dropped frames per 600; 4K costs ≈ 2 cores and 5.7 ms render per frame. Ships as one 121 MB GPL DLL that hard-imports `vulkan-1.dll`.
**Why:** mpv brings decoding, audio output, A/V sync, seeking and every container; the software render API works with wgpu as is. Display-synced timing removes the judder that audio-clock timing causes against our vsync (45–265 of 600 frames).
**Rejected:** `libmpv2` crate (only the OpenGL render API), audio-clock timing (judder), rendering at the video's own size (4K: ≈ 19 ms CPU per frame), GL interop (would need a WGL context beside wgpu), `ffmpeg-next` + `cpal`, GStreamer.

### 2026-10-27 · Viewer

**Decided:** the viewer (`app::viewer`) is `AppState::viewer`, drawn in an area over the window while the gallery keeps drawing below; it takes its keys at the start of the frame. Originals come from `app::originals`: two worker threads decode the wanted set (shown item, then neighbours nearest first) and create mipped `wgpu::Texture`s themselves; a request that leaves the set before it starts never runs. Zoom math, preloading order and control timing are pure functions in `domain::viewer`. The info panel and the gallery's info sheet share `app::info`; the viewer reports what it needs as `viewer::Action` (close, filter by tag, show in Explorer). `theme::motion` is off when Windows' animation effects are off.

### 2026-10-27 · Video playback in the viewer

**Decided:** the viewer talks to a `video::Playback`; the app's is `video::MpvPlayer`, UI tests use `video::FakePlayback`. `AppState::videos` makes the player when the first video shows, or holds why it can't (libmpv not found). All of the app's video `unsafe` is in `video::mpv`. Frames belong to a load: the UI counts the `loadfile`s it sent, the event thread counts mpv's `START_FILE`s, and frames show only after the `FILE_LOADED` of the latest load, so a video stepped away from never flashes. The UI reads mpv properties at most every 100 ms.
**Rejected:** egui's `Slider` for seek and volume (its rail doesn't show with our widget visuals), loading libmpv at startup.
