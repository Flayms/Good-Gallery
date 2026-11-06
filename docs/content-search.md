# Searching by what is in a photo (spike for #200)

Can Good Gallery find photos by what they show ("grass", "skyscrapers", "a dog on a beach") without tags, on the user's own computer, at 50k–500k photos? This page records what a spike measured on 2026-11-06 and proposes how to build it. Nothing here is built yet.

## Short answer

**Yes, and most of the pieces already exist.** A CLIP-style model turns each photo and each search text into 512 numbers, and photos whose numbers are close to the text's match it. `tract`, which already runs the face models, runs both halves of MobileCLIP-S0 unchanged. In a small test, every one of 13 searches put the right photo first.

## How it works

1. **Once per photo, in the background:** the image half of the model turns the photo into a vector of 512 numbers. It needs a 256 × 256 input, so it can read the **cached thumbnail** (up to 800 px wide) instead of the original. No original is decoded, and it works while a network share is offline.
2. **For each search:** the text half turns the search text into a vector of the same kind, in about 65 ms.
3. **Matching:** the photos are ranked by cosine similarity with the text's vector. Matches above a cutoff are shown best first, combined with the existing filters (library, folder, date, rating, tags).

## What the spike measured

Setup: `Xenova/mobileclip_s0` (ONNX export of Apple's MobileCLIP-S0: image half 43 MB, text half 161 MB, fp32), `tract-onnx` 0.23.9 as the app pins it, release build, on the 2-core dev VM. 12 photos from COCO val2017 with known content.

| | Time | Notes |
| --- | --- | --- |
| Load the image half | 0.46 s | once per session |
| Load the text half | 0.89 s | once, on the first search |
| One photo | 210–270 ms | 256 × 256, 2 cores; a typical 8-core desktop should be 3–4× faster |
| One search text | 63–84 ms | |

Ranking (cosine similarity of the best match, then the best wrong one):

| Search | First | Score | Best wrong |
| --- | --- | --- | --- |
| cats sleeping on a couch | the cats | 0.28 | 0.11 |
| a bear | the bear | 0.27 | 0.13 |
| skiing | the skier | 0.28 | 0.13 |
| a stop sign | the stop sign | 0.24 | 0.09 |
| a kitchen | the kitchen | 0.25 | 0.17 (living room) |
| a bedroom | the bedroom | 0.28 | 0.15 (living room) |
| baseball | the baseball game | 0.22 | 0.15 |
| surfing | the surfer | 0.27 | 0.13 |
| a river under a bridge | the river | 0.22 | 0.06 |
| a woman with her phone | the woman with a phone | 0.20 | 0.17 |
| a living room | the living room | 0.25 | 0.20 (bedroom) |
| children | the group of children | 0.13 | 0.12 |
| grass | the bear (standing on grass) | 0.11 | 0.10 |

**Reading it:** a concrete thing or scene scores 0.20–0.28 when it is the subject. Wrong photos stay below about 0.17, except related scenes (bedroom and living room). Weak or background concepts ("grass", "children" in a group photo) score low for everything. So a fixed cutoff of about 0.18 would show solid matches and hide the rest. A search where nothing passes could still show the best few, marked as weak. Twelve photos are far too few to set the cutoff. That needs a few hundred of the user's kind of photos, as `cargo xtask face-bench` does for faces.

## Costs at library scale

- **First pass:** 500k photos × 60–250 ms is about 8–35 hours of background work, depending on the computer. It runs like the face search (REQ-032-05): one photo at a time, paused while the user browses, resumed after a restart. 50k photos take 1–3.5 hours.
- **Storage:** 512 numbers per photo, 2 KB as f32 or 1 KB as f16. That's 0.5–1 GB in the index for 500k photos, or about 0.25 GB quantized to 8 bits, which costs little accuracy for cosine ranking.
- **Search:** comparing a query with 500k vectors is 256M multiply-adds, about 50–100 ms on one core. No search index (ANN) is needed at this size. The vectors (0.25–1 GB) can't all stay in memory on small machines, so they would be streamed from SQLite in blocks or kept as f16/int8 in RAM.
- **App size:** nothing is bundled. The models are downloaded on request, as face models are (spec 032), about 200 MB for MobileCLIP-S0.

## Pitfalls found

- **Padding the text with the end token makes every search the same.** The text half pools at the position of the highest token id, the end token. With end-token padding, every text gave nearly the same vector in `tract` (the pooling evidently picked a padding position rather than the first end token), and every search ranked the photos alike. MobileCLIP pads with 0, as open_clip does. (Also in `known-pitfalls.md`.)
- **The tokenizer must be ours.** It's CLIP's byte-level BPE (49,408 tokens, `</w>` at word ends, lowercase), about 150 lines of Rust plus the model's `tokenizer.json`. Adding the `tokenizers` crate would be the alternative, but it's large for one function.
- **MobileCLIP's preprocessing is not CLIP's.** It needs a 256 px center crop, 0–1, with no mean/std normalization (`preprocessor_config.json`). Other models differ, so like face models (REQ-032-03), each model carries its input style.

## Models

| Model | Size (fp32) | License | Note |
| --- | --- | --- | --- |
| MobileCLIP-S0 (Apple) | 43 + 161 MB | Apple AMLR (research use) | tested here; fastest |
| MobileCLIP-S2 (Apple) | larger | Apple AMLR | more accurate, slower; not tested |
| CLIP ViT-B/32 (OpenAI) | 335 + 242 MB (int8: 84 + 61 MB) | MIT | the classic; clear license; not tested, and whether `tract` runs the int8 files is open |

As with faces, the user picks and downloads a model; the app ships none.

## Proposal

A Tier 3 spec, **035 · Content search**, modeled on spec 032 (faces):

- **Off until a model is chosen** in a _Search_ section of the settings (list, download, SHA-256, add your own ONNX).
- **Background pass** over photos and video thumbnails, from the thumbnail cache, with progress in the processing icon, paused and resumed like the face search.
- **Vectors in the index** (f16 or int8, per model, never written to files), deleted with _Delete search data_.
- **The search bar searches by content** as well as by file name and tags. Results are best match first above a cutoff, combined with all filters, and can be kept as a smart album (spec 034).
- **Out of scope at first:** "find similar photos" from one photo (cheap to add later with the same vectors), languages other than English (needs a multilingual model), and a GPU.

Next step if agreed: the spec with its REQs, then a plan. The plan's first task measures MobileCLIP-S0 against CLIP ViT-B/32 on a few hundred labeled photos to choose the default model and the cutoff.
