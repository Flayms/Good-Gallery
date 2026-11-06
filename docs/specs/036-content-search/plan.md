# 036 · Content search · Plan

## Approach

Built as faces are (spec 032), and beside them rather than inside them: a content model is a different kind of thing (three files, a tokenizer, a text half) and its pass reads thumbnails, not originals.

- **`domain::content`:** what needs no IO. The model catalog as data (files with URL, SHA-256 and size, input side, mean and std, cutoff, license), CLIP's tokenizer (byte-level BPE from the model's `tokenizer.json`), the image preprocessing into the model's input, vectors quantized to 8 bits and their similarity.
- **`media::content`:** the engine, `tract` running a model's image and text halves, behind a `ContentNet` so tests run without the 200 MB files. **`media::content_store`:** `<data>/content-models/<id>/`, downloads of all files of a model with the hash check (`face_store`'s copying, shared).
- **`db::content`:** `content_vectors (media_id, model, size, mtime, vector)` (migration), the photos waiting for a model, progress, all vectors of a model in blocks, deleting.
- **`indexer::content`:** the background service, as `indexer::faces`: rounds of 20, kicks, pause, status, reachability. A photo's input is its cached 400 px thumbnail, else the file decoded at 512 px.
- **`app`:** the settings section, the processing icon, the search bar's suggestion and chip, `GallerySearch::content`, and a `ContentMatcher` the gallery's loader thread asks for the matching ids before it runs the search's query.

## Tasks

- [ ] 1. Spec, plan, `docs/content-search.md` (spike)
- [ ] 2. `domain::content`: catalog, tokenizer, preprocessing, quantized vectors and similarity (REQ-036-01, REQ-036-06)
- [ ] 3. `media::content`, `media::content_store`: engine, store, download (REQ-036-02); a test with the real model where it is on the machine
- [ ] 4. `db::content`: table, pending, progress, vectors, delete (REQ-036-03, REQ-036-04, REQ-036-08)
- [ ] 5. `indexer::content`: the service (REQ-036-04, REQ-036-08)
- [ ] 6. `GallerySearch::content`, the matcher in the gallery's loader, smart albums (REQ-036-05 … REQ-036-07)
- [ ] 7. Settings section, processing icon (REQ-036-01 … REQ-036-04)
- [ ] 8. Search bar: suggestion, chip, searching and empty states; snapshots (REQ-036-05, REQ-036-06)
- [ ] 9. Cutoffs measured on a few hundred labeled photos, as `face-bench` does for faces

## Decisions

### 2026-11-06 · MobileCLIP-S0 recommended, CLIP ViT-B/32 for a clear license

**Decided:** two listed models. MobileCLIP-S0 (Apple, 43 + 161 MB) is recommended: in the spike it put the right photo first for all 13 searches with the widest gap to wrong photos, at about 225 ms per photo and 65 ms per search on 2 cores. Its license is Apple's research license, which the section states. CLIP ViT-B/32 (OpenAI, MIT, 335 + 242 MB) runs as fast per photo in 32-bit float, but its scores lie closer together (it put the bedroom before the living room for "a living room"), so it gets a higher cutoff. It is there for a clear license.
**Rejected:** CLIP's 8-bit files (84 + 61 MB): `tract` runs them 6× slower than 32-bit (1.3 s per photo), and they need its value infos ignored to load. Larger models (MobileCLIP-S2, SigLIP): untested, and the pass over a large library is long already.

### 2026-11-06 · From the thumbnail cache

**Decided:** a photo's input is its cached 400 px thumbnail; only without one is the file decoded, at most 512 px. Videos use their thumbnail frame. The model reads 224–256 px, so nothing is lost, decoding a 24 MP JPEG (660 ms) would cost three times the model, and a library on an offline share can still be looked at.
**Rejected:** originals first (slower, needs the share), a thumbnail rendered for this purpose (a second pipeline).

### 2026-11-06 · Vectors as 8 bits, compared in memory per search

**Decided:** a vector is normalized, then stored as 512 signed bytes (512 bytes per photo, 256 MB for 500k). A search reads a model's vectors in blocks from the index and keeps the ids at or above the cutoff; 500k comparisons take well under a second, and the result is cached per words until vectors change. No search index (ANN).
**Rejected:** f32 or f16 (2–4× the space for no change in ranking at this cutoff), keeping all vectors in memory (256 MB at 500k for the whole session), an ANN library (a dependency for a size where scanning is fast enough).

### 2026-11-06 · Matches in the gallery's order, combined with the search

**Decided:** content is one more condition of `GallerySearch`, the words (`content`). The gallery's loader asks a `ContentMatcher` for the matching ids, then runs the usual query restricted to them, so sorting, date groups, every filter and smart albums work unchanged. The words are kept in smart albums; the matches are computed with the model chosen at the time.
**Rejected for now:** ranking by similarity (needs the order in SQL or a second gallery path; a cutoff keeps the matches good enough to scan by date), several content chips (rarely needed, and what "and" means for two phrases is unclear).

### 2026-11-06 · No model from a file

**Decided:** only listed models can be downloaded. A content model needs the tokenizer and the image input its maker used, which the app can't tell from the files.
**Rejected:** an _Add a model_ form with tokenizer, input size, mean and std: too much to ask, and a wrong guess gives silent nonsense (the padding pitfall in the spike).
