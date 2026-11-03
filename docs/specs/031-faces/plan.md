# 031 · Faces · Plan

## Approach

- **`domain::faces`:** what needs no IO. Boxes, landmarks, overlap, non-maximum suppression, the alignment transform, decoding each detector's output, the model catalog as data, cosine similarity, suggestions and groups.
- **`media::face_models`:** the engine (`tract`, behind `FaceEngine`), running a detector or a recognizer as `domain` describes it, the model folder, downloads.
- **`media::faces`:** decoding a photo upright, the detection passes, writing regions with exiftool.
- **`db`:** `detected_faces`, `face_embeddings`, `face_rejections`, `faces_scanned` (migration), one more write connection as the thumbnail service has.
- **`indexer::faces`:** the background service (REQ-031-05, REQ-031-16).
- **`app`:** the settings section, the viewer's boxes and actions, the People page.

## Tasks

- [ ] Spec, `docs/face-models.md`, REQ-011-11 deprecated
- [ ] `domain::faces`: geometry, decoding, alignment, catalog, similarity (REQ-031-06, REQ-031-07)
- [ ] `media::face_models`: engine, store, download (REQ-031-02, REQ-031-03, REQ-031-06, REQ-031-07)
- [ ] `db` tables and queries (REQ-031-04, REQ-031-05, REQ-031-16)
- [ ] `indexer::faces` (REQ-031-05, REQ-031-08, REQ-031-16)
- [ ] Settings section (REQ-031-01 … REQ-031-04, REQ-031-17)
- [ ] Writing regions (REQ-031-12 … REQ-031-14)
- [ ] Viewer (REQ-031-09 … REQ-031-13)
- [ ] People page and groups (REQ-031-15)
- [ ] Benchmark: speed and memory for `docs/face-models.md`, thresholds per model

## Decisions

### 2026-11-03 · The user picks the models, and none is bundled

**Decided:** detection and recognition models are chosen in the settings. Every model arrives the same way, by download or by a file the user has, and works offline afterwards. A recipe (input size, color order, scaling, threshold, output format) is data in `domain`, and a model's identity is its file's SHA-256, so a model the catalog doesn't name works without any reference to it in the code. Recognition models differ only in their recipe. Detection models differ in their output, so each output format has its own decoder.
**Rejected:** one fixed pair (users disagree on size against accuracy, and licenses differ). Bundling YuNet (232 KB): recognition needs a download anyway, so it saves one click and adds a second path.

### 2026-11-03 · No more network promise

**Decided:** REQ-011-11 is deprecated. Models are downloaded with `ureq` and `rustls` from the desktop app; the browser version still sends nothing (REQ-019-12). An added link may point anywhere, because the user chose it. A listed model's hash is built in, so a host can't swap it.
**Rejected:** running `curl` as a helper (not on every system, and progress and cancelling need parsing), keeping the promise with file-only adding.

### 2026-11-03 · `tract`, behind an interface

**Decided:** models run on `tract`, already in the app, on the CPU. `FaceEngine` hides it, so ONNX Runtime can be added if the benchmark shows a model that won't load or needs a graphics card.
**Rejected for now:** ONNX Runtime through `ort`: a native library per platform (10–79 MB archives), C++ parsing files users pick, and a crate still at a release candidate. Spec 022 rejected it for the same reasons.

### 2026-11-03 · Files hold names, the index holds the rest

**Decided:** only a name the user confirmed reaches a file. A detected face overlapping a named region is that person, worked out when read, so another program's change to the regions needs no sync. Face tables live in the index, not a database of their own, so they follow `media` by foreign key and share its writer conventions. `face_embeddings` is its own table and read only by the face code, so the gallery's reads don't pay for it.
**Rejected:** writing unknown faces to files (other programs show them as nameless boxes, and digiKam's writes on rejecting caused complaints, its bug 511184). A separate database (cleanup without foreign keys, for rejections to survive a rebuild, which is rare).

### 2026-11-03 · A first look, and more only where it hints at faces

**Decided:** every photo gets one look at 640 × 640. Without a confident face it gets another at half the size, for a face larger than YuNet is trained on. Where even a weak candidate (`HINT_SCORE` 0.3) is small, overlapping tiles follow. Real YuNet missed a 110 px face on a 4000 px photo until the hints were weak ones.
**Rejected:** tiles on every photo (about five times the work for a library that mostly has large faces or none), a confident first detection as the trigger (the case above).

### 2026-11-03 · Taking a face out of a group ignores it

**Decided:** a face taken out of a group becomes ignored, and comes back with _Show ignored faces again_. Groups are computed, so there is nothing to remove it from, and it would rejoin the same group if merely skipped.
**Rejected:** a rejection list per group: groups have no lasting identity.

### 2026-11-03 · Unnamed regions of other programs stay out

**Decided:** only named regions are read (REQ-004-13). Unnamed ones stay in the file, untouched, and the detector usually finds the same face.
**Rejected:** importing them as unknown faces: a schema change and a second kind of unknown face, for a case rarely seen.

### 2026-11-03 · Unsettled licenses are stated, not avoided

**Decided:** YuNet, SFace, RetinaFace and AuraFace have a download button. EdgeFace and `buffalo_l` are not in the code: the info page describes them, with a disclaimer for `buffalo_l`, and they are added by link or file. RetinaFace's and AuraFace's open questions are on the page. Only AuraFace's `glintr100.onnx` is downloaded, since its repository also holds InsightFace's non-commercial files.
**Rejected:** download buttons for models whose publisher says non-commercial.
