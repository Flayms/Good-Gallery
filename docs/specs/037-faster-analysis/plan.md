# 037 · Faster face and content analysis · Plan

## Approach

Measure first, then run the models on several photos at once, keeping the services' shape (`indexer::faces`, `indexer::content`: rounds, kicks, pause, status).

- **`indexer::analysis`:** how many threads a pass may take (`threads()`: half the cores, at least one) and `map_in_order`, which spreads work over scoped threads and returns the results in the items' order. A panic goes on in the pass's thread, where each pass already catches it.
- **Faces:** `FaceWork` gets `batch`, `detect_many` and `recognize_many`, sequential by default (the tests' fakes). A round takes its photos in batches of `batch()`. `RunModels` decodes and detects a batch's photos at once, each on its own thread with the one loaded model (`tract`'s plan is shared, each run has its own state). It keeps the batch decoded for the recognition right after (REQ-032-23), so at most one batch of photos is held in memory. Results are stored in the order one at a time would store them.
- **Content:** `ContentWork` gets `batch` and `vectors`; `RunContent` runs the image half on a batch's items at once. Each batch is stored in one transaction.
- **Stopping:** both passes check for a kick, a pause or the end between batches. A batch in flight is finished and stored; the rest stays waiting (REQ-037-04).
- **Benchmark:** `cargo xtask analysis-bench faces|content …` runs a pass over a folder of photos into a scratch index, one at a time and then several at once, and says whether both gave the same results.

## Tasks

- [x] 1. `xtask analysis-bench`: both passes, one at a time against several, same-results check (REQ-037-01, REQ-037-02)
- [x] 2. Measure where the time goes (below); `tract`'s own `multithread-mm` not used (Decisions)
- [x] 3. Faces: batches, `RunModels` on several threads, recognition from the kept batch (REQ-037-01, REQ-037-02, REQ-037-04)
- [x] 4. Content: batches, `RunContent` on several threads, a transaction per batch (REQ-037-01, REQ-037-02, REQ-037-04)
- [x] 5. At most half the cores (REQ-037-03)
- [x] 6. Benchmark, results below, `docs/architecture.md` (threads table)
- [ ] 7. Benchmark on a 4- and an 8-core machine with real photos, and measure frame times with a pass running (REQ-037-03)

## Measurements

On the development machine: 2 cores, 3 GB, a load of about 1 from other programs; debug build of `xtask`. Twelve 3000 × 2000 JPEGs (the face fixture, scaled and tinted).

| Pass | One at a time | Two at once | Same results |
| --- | --- | --- | --- |
| Faces, YuNet | 25.0 s, 0.48 photos/s | 14.8 s, 0.81 photos/s (1.69×) | yes |
| Content, MobileCLIP-S0 | 33.3 s, 0.36 photos/s | 19.7 s, 0.61 photos/s (1.69×) | yes |

Decoding one of these photos takes 126 ms, YuNet's look at it about 2 s (several passes over the photo for small faces).

The app takes half the cores, so on this machine it still looks at one photo at a time; with 4 cores it takes two at once, with 8 four.

## Decisions

### 2026-11-06 · Several photos through the model, not decoding ahead

**Decided:** each pass decodes and looks at several photos at once, as many as half the cores.
**Rejected:** a pool that decodes the next photos while the model runs on the current one, which this plan first proposed. Measured, it was 20% *slower* (30.4 s against 25.1 s above): the model, not decoding, takes most of the time (2 s against 126 ms), so there is little to overlap, and the decoder competes with the model for the cores. The earlier figures (660 ms to decode, 90 ms for YuNet) were for a 24 MP photo and one pass of the model.
**Rejected:** `tract`'s `multithread-mm` feature (rayon in matrix products). One photo would use several cores, but it adds a global thread pool, and splitting products may change the order of sums; several photos at once keeps every result computed exactly as before.

### 2026-11-06 · Half the cores, no lower thread priority

**Decided:** a pass takes at most half the cores. The other half is left to the UI, scanning and thumbnails.
**Rejected:** lowering the threads' priority: the standard library has no way to, and doing it per platform (`SetThreadPriority`, `nice`) adds code for each, with no measured need yet (task 7).

### 2026-11-06 · CPU threads first, no graphics card

**Decided:** use the cores with the engine we have. `tract` is pure Rust, so nothing new ships.
**Rejected for now:** ONNX Runtime with DirectML (Windows) or CUDA. It would run the models several times faster, but adds a native library of 10–20 MB per platform to bundle and sign, its own failure modes on old drivers, and results that can differ from the CPU's in the last bits (REQ-037-01 would become "nearly the same"). Worth a second look once task 7 shows how far the cores get.

### 2026-11-06 · No lower-resolution decoding

**Decided:** originals are still decoded at up to 8000 px for the faces pass.
**Rejected:** JPEG's scaled decoding (1/2, 1/4, 1/8) would cut decoding time, but the detector's later passes over a photo's tiles find small faces only at full resolution, so it would find fewer faces. It also matters little now that decoding is a small part of the time.
