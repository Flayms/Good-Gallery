# M09 · Video

**Status:** Done (approved 2026-10-27, self-approved: the user asked to work through the milestones without their input)
**Specs:** REQ-009-06, and the video parts of REQ-009-02 (← / → seek while the video controls have focus), REQ-009-04 (the original file), REQ-013-10 (paused videos keep the controls)

## Approach

**Player (`app::video`).** The M0b spike's libmpv binding (`mpv.rs`: runtime-loaded `libmpv-2.dll`, own FFI for the software render API) moves into the app with its unsafe code kept in that one module. `MpvPlayer` owns an mpv core with `vo=libmpv`, `hwdec=auto-copy`, `keep-open=yes`, `idle=yes`, `terminal=no`; a render thread renders `rgb0` at the size the video takes on screen (never larger than the video) and, as for thumbnails and originals, creates and fills the `wgpu::Texture` itself; an event thread records loads, ends and errors. Timing is display-synced as measured in M0b (`video-sync=display-resample`, the UI wakes the render thread once per presented frame, which calls `report_swap` and skips repeats); `override-display-fps` is the monitor's refresh rate (`media::helpers::display_refresh_rate`, 60 if unknown). Without a wgpu render state (tests) the render thread hands decoded frames to the UI as images and timing stays with mpv's audio clock (`block_for_target_time`). mpv property reads take the core lock, so the UI reads the playback state at most every 100 ms.

**Playback trait.** The viewer talks to a `Playback` (load, stop, pause, seek, volume, mute, state, frame); the app gets an `MpvPlayer`, UI tests a recording fake, so the controls are tested without libmpv and without timing. A separate test plays generated fixtures in all four containers (MP4 H.264 + AAC, MOV HEVC + AAC rotated, MKV H.264 + Opus, WebM VP9 + Opus) through the real `MpvPlayer` without audio output (exit criterion); it is skipped without libmpv or ffmpeg unless `GG_REQUIRE_HELPERS` is set. The player is made on first use (the DLL is 121 MB); `libmpv-2.dll` is found like ffmpeg: `GOOD_GALLERY_MPV`, `mpv\libmpv-2.dll` next to the app, `PATH`. Without it, videos show their preview and say why.

**Viewer.** A video item loads its original into the player and plays (REQ-009-06); its preview (thumbnail) stays until the first frame. The frame is fitted like a photo (rotation from `video-params/rotate`: the software renderer ignores it, the quad is drawn rotated) and doesn't zoom. Controls in a glass bar along the bottom of the video, like the browser's: play / pause, time "0:12 / 1:05", a seek slider (keyframe seeks while dragging, an exact one on release), mute and volume. They fade with the viewer's controls; a paused video keeps them (REQ-013-10). Keys: Space plays / pauses; ← / → seek 5 s while the video controls have focus (the last click was on them), else they step (REQ-009-02). Stepping away or closing stops playback. Volume and mute stay for the session.

Added while implementing: a click on the video plays / pauses it (as in a browser; beside it, it closes), ↑ / ↓ change the volume in 5 % steps while the controls have focus, play at the end starts over (`keep-open` pauses there), and the seek and volume sliders are own thin sliders (egui's rail doesn't show with our visuals). The player is told the render size from the size the video took in the last frame.

Differences from the reference: the browser's `<video controls>` has a fullscreen and a download button and a menu; not ported (fullscreen is out of scope in spec 009). Seeking before the file has loaded is mpv's own (it reads the container's index; local and network files alike).

## Reference

- `src/renderer/src/components/media-viewer.tsx`: `<video controls autoPlay>`, `onPlay` / `onPause` keep the controls, a new video starts playing, arrows seek while the video has focus.
- `spikes/video` (M0b): FFI, render and event threads, display-synced timing, rotation, seek modes; `fixtures.sh` for the fixtures.

## Tasks

- [x] 1. `domain::video`: time format, seek step and clamping, volume steps, rotation quarters and fitted size (REQ-009-06)
- [x] 2. `app::video::mpv` (FFI) and `MpvPlayer` with render and event threads, GPU and image paths; `media::helpers::{find_mpv, display_refresh_rate}`
- [x] 3. Fixture test: all four containers play through `MpvPlayer` (exit criterion)
- [x] 4. `Playback` trait, `AppState` video slot (lazy, with the reason when unavailable), fake for tests
- [x] 5. Viewer: play the shown video, draw its frame, video controls, keys, focus rule, stop on step and close (REQ-009-06, REQ-009-02, REQ-013-10)
- [x] 6. UI tests, snapshot, running-app check with real videos
- [x] 7. Docs: specs, architecture, pitfalls, progress

## Exit criteria

From `docs/rewrite/plan.md`: fixture videos in all four containers play; REQs pass.
