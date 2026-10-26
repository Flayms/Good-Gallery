# Architecture

Target sketch from `docs/rewrite/plan.md`; this file becomes the source of truth as milestones settle it. Keep it describing the current state, not history.

```
crates/
  domain    pure types and logic: media, tags, search model, gallery layout, date groups, zoom math. No IO.
  db        rusqlite, migrations (SQL taken from the reference), queries incl. tag search
  media     thumbnail rendering, video frames (ffmpeg), exiftool process, thumbhash
  indexer   scan queue, walk, watcher, writer, reachability, rescan scheduling
  app       eframe binary: UI, texture streaming, viewer, settings
xtask/      check-specs, fixture library generator, release
```

## Decisions

Architectural decisions that outlive a milestone, newest last. One entry each: what was decided, why (with measurements where there are any), what was rejected, and the milestone plan with the details. Decisions that only matter within one milestone stay in that milestone's Approach section.

_None yet._
