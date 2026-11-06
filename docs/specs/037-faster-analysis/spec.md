# 037 · Faster face and content analysis

**Status:** Draft

## Goal

Looking for faces (spec 032) and making photos searchable by what they show (spec 036) go through a whole library in the background, one photo at a time on one thread. On a library of 100,000 photos that takes days, while most of the computer's cores wait. The same results should come in a fraction of the time, using the cores the computer has, without finding fewer faces or ranking photos differently (#224).

## Requirements

### REQ-037-01 · The same results

- **Given** a library analyzed with the same models before and after this change
- **When** the passes are done
- **Then** every photo has the same faces, with the same boxes and landmarks, and the same face and content vectors (equal after quantization, REQ-036-04), as with one photo at a time

### REQ-037-02 · Several photos at once

- **Given** a computer with more than two cores and photos waiting for a pass
- **When** the pass runs
- **Then** reading and decoding the next photos overlaps with running the models on the current ones, and the models run on several photos at once
- **And** on 4 cores the faces pass and the content pass each go through a test set of photos at least twice as fast as one photo at a time (measured by `cargo xtask analysis-bench`, recorded in the plan)

### REQ-037-03 · Browsing stays smooth

- **Given** a pass running on all the threads it may use
- **When** the user scrolls the gallery or opens the viewer
- **Then** frame times stay within REQ-006-12's budget, and the pass's threads run at a lower priority than the UI and thumbnail rendering
- **And** it uses at most half the cores, at least one, leaving the rest to scanning and thumbnails

### REQ-037-04 · Stopping and pausing as before

- **Given** a pass with several photos in flight
- **When** the user pauses it, changes the model, removes a library or quits
- **Then** it stops within a second, keeps what was finished, and photos in flight are done again later, as one photo at a time did (REQ-032-16, REQ-036-08)

## Out of scope

- Running models on the graphics card (a different engine; decided in the plan).
- Faster or smaller models, or models at a lower precision: they change the results.
- Decoding photos at a lower resolution for detection: small faces would be missed.
- A setting for the number of threads.

## Assumptions

- The models' engine (`tract`) gives the same output for the same input whichever thread runs it.
- Most of the faces pass is reading and decoding originals (about 660 ms for a 24 MP JPEG, against 90 ms for YuNet), most of the content pass is the model (about 225 ms per thumbnail with MobileCLIP-S0 on 2 cores).

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-037-01 | —    | pending |
| REQ-037-02 | —    | pending |
| REQ-037-03 | —    | pending |
| REQ-037-04 | —    | pending |
