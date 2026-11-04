# 032 · Faces · Plan

## Approach

- **`domain::face_models`:** what needs no IO. Boxes, landmarks, overlap, non-maximum suppression, the alignment transform, decoding each detector's output, the model catalog as data, cosine similarity, suggestions and groups.
- **`media::face_models`:** the engine (`tract`, behind `Net`), running a detector or a recognizer as `domain` describes it.
- **`media::face_store`:** the model folder, downloads.
- **`media::face_regions`:** writing regions with exiftool.
- **`db`:** `detected_faces`, `face_embeddings`, `face_rejections`, `faces_scanned` (migration), one more write connection as the thumbnail service has.
- **`indexer::faces`:** the background service (REQ-032-05, REQ-032-16).
- **`app`:** the settings section, the viewer's boxes and actions, the People page.

## Tasks

- [x] Spec, `docs/face-models.md`, REQ-011-11 deprecated
- [x] `domain::face_models`: geometry, decoding, alignment, catalog, similarity (REQ-032-06, REQ-032-07)
- [x] `media::face_models`, `media::face_store`: engine, store, download (REQ-032-02, REQ-032-03, REQ-032-06, REQ-032-07)
- [x] `db::faces` tables and queries (REQ-032-04, REQ-032-05, REQ-032-16)
- [x] `indexer::faces` (REQ-032-05, REQ-032-08, REQ-032-16)
- [x] Settings section (REQ-032-01 … REQ-032-04, REQ-032-17)
- [x] `media::face_regions`: writing regions (REQ-032-12 … REQ-032-14)
- [x] Viewer (REQ-032-09 … REQ-032-13)
- [x] People page and groups (REQ-032-15)
- [x] Benchmark (`cargo xtask face-bench`): speed and memory for `docs/face-models.md`, thresholds per model, on LFW and WIDER FACE
- [ ] Detection and recognition rates on a real library

## Decisions

### 2026-11-03 · The user picks the models, and none is bundled

**Decided:** detection and recognition models are chosen in the settings. Every model arrives the same way, by download or by a file the user has, and works offline afterwards. A recipe (input size, color order, scaling, threshold, output format) is data in `domain`, and a model's identity is its file's SHA-256, so a model the catalog doesn't name works without any reference to it in the code. Recognition models differ only in their recipe. Detection models differ in their output, so each output format has its own decoder.
**Rejected:** one fixed pair (users disagree on size against accuracy, and licenses differ). Bundling YuNet (233 KB): recognition needs a download anyway, so it saves one click and adds a second path.

### 2026-11-03 · No more network promise

**Decided:** REQ-011-11 is deprecated. Models are downloaded with `ureq` and `rustls` from the desktop app; the browser version still sends nothing (REQ-019-12). An added link may point anywhere, because the user chose it. A listed model's hash is built in, so a host can't swap it.
**Rejected:** running `curl` as a helper (not on every system, and progress and cancelling need parsing), keeping the promise with file-only adding.

### 2026-11-03 · `tract`, behind an interface

**Decided:** models run on `tract`, already in the app, on the CPU. `Net` hides it, so ONNX Runtime can be added if the benchmark shows a model that won't load or needs a graphics card.
**Rejected for now:** ONNX Runtime through `ort`: a native library per platform (10–79 MB archives), C++ parsing files users pick, and a crate still at a release candidate. Spec 022 rejected it for the same reasons.

### 2026-11-03 · Files hold names, the index holds the rest

**Decided:** only a name the user confirmed reaches a file. A detected face overlapping a named region is that person, worked out when read, so another program's change to the regions needs no sync. Face tables live in the index, not a database of their own, so they follow `media` by foreign key and share its writer conventions. `face_embeddings` is its own table and read only by the face code, so the gallery's reads don't pay for it.
**Rejected:** writing unknown faces to files (other programs show them as nameless boxes, and digiKam's writes on rejecting caused complaints, its bug 511184). A separate database (cleanup without foreign keys, for rejections to survive a rebuild, which is rare).

### 2026-11-03 · Thresholds keep strangers out

**Decided:** a recognition threshold is the lowest at which about 99% of a group is one person and at most 0.5% of strangers get a suggestion, in a simulation with the app's own `suggest` and `group` on LFW: SFace 0.50, AuraFace 0.45 (where it still finds only 89%), and 0.50 for a model the catalog doesn't know. YuNet's detection score stays 0.8 (2.8 false boxes per 100 WIDER FACE photos), RetinaFace's rises from 0.7 to 0.95 (from 19.5 to 5.6). The catalog's numbers beat those in a model's note and the stored setting, so a measurement reaches models already on a computer.
**Rejected:** thresholds from a rate of false matches per pair, as usually published: SFace's 0.363 (0.1% per pair) compares a face with every known face, so most strangers were named at 0.30, and 12% still at 0.40. Calibrating on a real library, which this machine doesn't have.

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
