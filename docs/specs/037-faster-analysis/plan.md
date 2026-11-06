# 037 · Faster face and content analysis · Plan

## Approach

Measure first, then parallelize where the time goes, keeping the services' shape (`indexer::faces`, `indexer::content`: rounds, kicks, pause, status).

- **Benchmark:** `cargo xtask analysis-bench <folder>` runs both passes over a folder of photos into a scratch index, once with one worker and once with the new pipeline, and prints photos per second, the share of time in reading, decoding and the models, and whether the results are identical (REQ-037-01). Without the models on the machine it says so and stops.
- **Faces: a decode pipeline.** A small pool (`workers = max(1, cores / 2)`) reads and decodes the next photos of the round (`media::original::load_original`) into a bounded queue of at most `workers + 1` decoded photos, so memory stays bounded (an 8000 px photo is 256 MB as RGBA; the queue holds them at the size they are decoded today). The model thread takes them in order and runs detection and recognition as today. Results are written in one transaction per round instead of one per photo.
- **Content: several model workers.** The model is loaded once (`tract`'s optimized plan is immutable and `Send + Sync`); each worker thread keeps its own run state and takes thumbnails from the round. Vectors are written per round.
- **Priority:** the threads lower their own priority as the thumbnail workers do (REQ-006-15).
- **Stopping:** every worker checks the shared control between photos; a round in flight is dropped, its unfinished photos stay pending (REQ-037-04).

## Tasks

- [ ] 1. `xtask analysis-bench`: timings split by read, decode, model; identical-results check. Record the baseline here (REQ-037-01, REQ-037-02)
- [ ] 2. Check whether `tract` 0.23.9 runs one model on several threads by itself (its `multithread` executor); if it does and scales, the content pass needs only that (REQ-037-02)
- [ ] 3. Faces: decode pool and bounded queue, writes per round (REQ-037-01, REQ-037-02, REQ-037-04)
- [ ] 4. Content: shared plan, several workers, writes per round (REQ-037-01, REQ-037-02, REQ-037-04)
- [ ] 5. Thread priority and core limit; frame-time check with a pass running (REQ-037-03)
- [ ] 6. Benchmark again, record the results here, update `docs/architecture.md` (threads table)

## Decisions

### 2026-11-06 · CPU threads first, no graphics card (proposed)

**Decided:** use the cores with the engine we have. Both passes are bound by work that parallelizes well (decoding for faces, the model for content), and `tract` is pure Rust, so nothing new ships.
**Rejected for now:** ONNX Runtime with DirectML (Windows) or CUDA. It would run the models several times faster, but adds a native library of 10–20 MB per platform to bundle and sign, its own failure modes on old drivers, and results that can differ from the CPU's in the last bits (REQ-037-01 would become "nearly the same"). Worth a second look once task 6 shows how far the CPU gets.

### 2026-11-06 · No lower-resolution decoding (proposed)

**Decided:** originals are still decoded at up to 8000 px for the faces pass.
**Rejected:** JPEG's scaled decoding (1/2, 1/4, 1/8) would cut decoding time a lot, but the detector's later passes over a photo's tiles find small faces only at full resolution, so it would find fewer faces.
