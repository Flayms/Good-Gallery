# Progress

The current milestone is the first row that isn't `Done`. Statuses: `Not started` → `Planning` (plan drafted, waiting for approval) → `In progress` → `Done`. `Blocked` needs a decision by the user (say which in the log).

| Milestone | Title                                  | Status      | Plan                                    |
| --------- | -------------------------------------- | ----------- | --------------------------------------- |
| M0a       | Spikes: grid and look                  | Done        | [M00](rewrite/milestones/M00-spikes.md) |
| M0b       | Spikes: video                          | Planning    | [M00](rewrite/milestones/M00-spikes.md) |
| M1        | Foundation                             | Not started | —                                       |
| M2        | Spec audit                             | Not started | —                                       |
| M3        | Database and libraries                 | Not started | —                                       |
| M4        | Indexing, change detection, metadata   | Not started | —                                       |
| M5        | Thumbnails                             | Not started | —                                       |
| M6        | Gallery                                | Not started | —                                       |
| M7        | Search and sidebar                     | Not started | —                                       |
| M8        | Photo viewer and filmstrip             | Not started | —                                       |
| M9        | Video                                  | Not started | —                                       |
| M10       | Settings, design polish, glass         | Not started | —                                       |
| M11       | Packaging and release                  | Not started | —                                       |

## Log

Newest first. One entry per session: date, milestone, what was done, decisions (lasting ones also in `docs/architecture.md`), open issues, next step.

### 2026-10-26 · M0a · Spikes: grid and look

- M0 plan written and approved, split into M0a (grid + look, `spikes/grid`) and M0b (video). Thumbnail cache copied to `%TEMP%\gg-spike\thumbs` (66 096 files, 3.7 GB); the original wasn't opened.
- **Grid: Go.** p95 frame interval 7.2 ms at 12 columns over 100k items (7.3 ms at 250k), vsync off, integrated AMD GPU. Keys: worker-side texture creation, 4 low-priority decode threads, libwebp, own mipmapped textures, LRU that keeps the prefetch band.
- **Look: Go, approved by the user from screenshots.** Glass (dual Kawase in a paint callback) matches Chromium within 1–2 color levels at ≈ 0.4 ms per surface; Atkinson Hyperlegible Next with a baked `tnum` copy; title bar variant 3 (own buttons, native hit-test codes) works.
- Decisions in `docs/architecture.md`; pitfalls added (Defender first-read cost, vsync and p95, egui 0.37 API notes, …).
- Open: (1) manual check that snap layouts show when hovering the maximize button (`cargo run --release` in `spikes/grid`); (2) thumbnail cache layout vs Defender, to decide in M5; (3) fast flings at 12 columns outrun decoding, handle in M6.
- Next: M0b (video spike), `docs/rewrite/milestones/M00-spikes.md` tasks 10–13.

### 2026-10-26 · Setup

- Repo created next to the reference (`../good-gallery`). AGENTS.md, master plan, progress log, architecture stub, known pitfalls, glossary and design copies, all 17 specs ported (requirements unchanged, Verification tables reset, statuses `Approved`), `.claude/settings.json` (reference readable, not editable).
- Open: MSVC C++ build tools aren't installed yet, so nothing links (see `docs/known-pitfalls.md`). The user installs them before M0.
- Next: M0, write `docs/rewrite/milestones/M00-spikes.md`.
