# Progress

The current milestone is the first row that isn't `Done`. Statuses: `Not started` → `Planning` (plan drafted, waiting for approval) → `In progress` → `Done`. `Blocked` needs a decision by the user (say which in the log).

| Milestone | Title                                  | Status      | Plan |
| --------- | -------------------------------------- | ----------- | ---- |
| M0        | Spikes (grid, video, look)             | Not started | —    |
| M1        | Foundation                             | Not started | —    |
| M2        | Spec audit                             | Not started | —    |
| M3        | Database and libraries                 | Not started | —    |
| M4        | Indexing, change detection, metadata   | Not started | —    |
| M5        | Thumbnails                             | Not started | —    |
| M6        | Gallery                                | Not started | —    |
| M7        | Search and sidebar                     | Not started | —    |
| M8        | Photo viewer and filmstrip             | Not started | —    |
| M9        | Video                                  | Not started | —    |
| M10       | Settings, design polish, glass         | Not started | —    |
| M11       | Packaging and release                  | Not started | —    |

## Log

Newest first. One entry per session: date, milestone, what was done, decisions (lasting ones also in `docs/architecture.md`), open issues, next step.

### 2026-10-26 · Setup

- Repo created next to the reference (`../good-gallery`). AGENTS.md, master plan, progress log, architecture stub, known pitfalls, glossary and design copies, all 17 specs ported (requirements unchanged, Verification tables reset, statuses `Approved`), `.claude/settings.json` (reference readable, not editable).
- Open: MSVC C++ build tools aren't installed yet, so nothing links (see `docs/known-pitfalls.md`). The user installs them before M0.
- Next: M0, write `docs/rewrite/milestones/M00-spikes.md`.
