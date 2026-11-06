# 037 · Faster face and content analysis

**Status:** Implemented

## Goal

Looking for faces (spec 032) and making photos searchable by what they show (spec 036) go through a whole library in the background, one photo at a time on one thread. On a library of 100,000 photos that takes days, while most of the computer's cores wait. The same results should come in a fraction of the time, using the cores the computer has, without finding fewer faces or ranking photos differently (#224).

## Requirements

### REQ-037-01 · The same results

- **Given** a library analyzed with the same models before and after this change
- **When** the passes are done
- **Then** every photo has the same faces, with the same boxes, and the same face and content vectors (equal after quantization, REQ-036-04), as with one photo at a time

### REQ-037-02 · Several photos at once

- **Given** a computer with four cores or more, and photos waiting for a pass
- **When** the pass runs
- **Then** the faces pass and the content pass each decode and look at several photos at once, as many as REQ-037-03 allows, and write them in the order one at a time would
- **And** with two at once, each goes through a test set of photos at least 1.5 times as fast as one at a time (measured by `cargo xtask analysis-bench`, recorded in the plan)

### REQ-037-03 · Browsing stays smooth

- **Given** a pass running on all the threads it may use
- **When** the user scrolls the gallery or opens the viewer
- **Then** frame times stay within REQ-006-12's budget
- **And** each pass takes at most half the cores, at least one, leaving the rest to the UI, scanning and thumbnails

### REQ-037-04 · Stopping and pausing as before

- **Given** a pass with several photos in flight
- **When** the user pauses it, changes the model, removes a library or quits
- **Then** it stops once the photos in flight are done, as it did after the one photo in flight, keeps what was finished, and the rest waits for later (REQ-032-16, REQ-036-08)

## Out of scope

- Running models on the graphics card (a different engine; decided in the plan).
- Decoding photos ahead of the models (measured, decided in the plan).
- Faster or smaller models, or models at a lower precision: they change the results.
- Decoding photos at a lower resolution for detection: small faces would be missed.
- A setting for the number of threads.

## Assumptions

- The models' engine (`tract`) gives the same output for the same input whichever thread runs it.
- Most of either pass is its model: YuNet's passes over a 6 MP photo take about 2 s against 126 ms to decode it, MobileCLIP-S0 most of 2.8 s per photo (debug build, see the plan).

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-037-01 | `crates/indexer/src/faces.rs` › req_037_01_several_at_once_find_the_same_faces_with_the_real_model; `crates/indexer/src/content.rs` › req_037_01_several_at_once_store_the_same_vectors; `crates/indexer/src/analysis.rs` › req_037_01_keeps_the_order_of_the_items | partial: real face boxes and, in the benchmark, real content vectors; face vectors only with a fake (no recognition model in the repository) |
| REQ-037-02 | `crates/indexer/src/faces.rs` › req_037_02_looks_at_several_photos_at_once; `crates/indexer/src/analysis.rs` › req_037_02_runs_on_several_threads | pass, 1.69 times as fast for both passes with two at once (plan) |
| REQ-037-03 | `crates/indexer/src/analysis.rs` › req_037_03_takes_at_most_half_the_cores | partial: the frame times with a pass running aren't measured |
| REQ-037-04 | `crates/indexer/src/faces.rs` › req_037_04_stops_between_batches; `crates/indexer/src/content.rs` › req_037_04_stops_between_batches | pass |
