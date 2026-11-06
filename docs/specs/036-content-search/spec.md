# 036 · Content search

**Status:** Implemented

## Goal

Most photos carry no tags, and nobody tags 100,000 photos by hand. Users still remember what is in a photo: a beach, a dog, skyscrapers at night. Content search finds photos by what they show, from a few words typed into the search bar, on the user's own computer, with a model the user chooses (#200). The spike behind this spec is in [`docs/content-search.md`](../../content-search.md).

## Requirements

### REQ-036-01 · Choosing a model

- **Given** the _Content search_ section of the settings
- **When** the user opens it
- **Then** it says in a sentence what content search does, that it runs on this computer, and that choosing a model starts going through the photos in the background
- **And** it lists the models with their download size, speed and license, one marked as recommended, each with what it is there for
- **And** a model on this computer can be chosen and removed; a chosen model is _In use_

### REQ-036-02 · Downloading a model

- **Given** a listed model that isn't on this computer
- **When** the user chooses _Download_
- **Then** its files download in the background with visible progress, without slowing browsing, and can be cancelled
- **And** once every file's SHA-256 matches the one built into the app, the model is on this computer, also offline
- **And** a failed, cancelled or mismatching download leaves nothing behind, and the app says why

### REQ-036-03 · Off until chosen

- **Given** no model chosen
- **When** the app runs
- **Then** it looks at no photos for content and stores no content data, and the search bar offers no content search
- **And** what content search computes stays in the index, never in files, and _Delete search data_ in the settings removes all of it

### REQ-036-04 · Going through the photos in the background

- **Given** a chosen model and photos and videos it hasn't looked at
- **When** the app is running
- **Then** it works through them one by one on its own thread, newest first, from their cached thumbnails, and from the file only where no thumbnail is cached
- **And** it carries on after a restart, skips what it can't read until the file changes, and looks again at a file that changed
- **And** the section shows how far it is (`1,234 of 50,000 done`), with _Pause_ and _Resume_, and the processing icon shows it while it works (REQ-032-20)
- **And** a library that is offline is skipped while its thumbnails are missing, and taken up when it is back

### REQ-036-05 · Searching by content

- **Given** a chosen model that has looked at some photos
- **When** the user types a few words into the search bar
- **Then** the first suggestion is _Photos showing “…”_ with the typed words, and choosing it adds a content chip to the search
- **And** the gallery shows the photos and videos that match the words, in the gallery's sort order, combined with every other filter of the search
- **And** a search holds one content chip; another replaces it, and removing the chip removes the condition
- **And** while the model reads the words, the gallery shows that it is searching, and a search that matches nothing says so

### REQ-036-06 · What counts as a match

- **Given** a content search
- **When** the photos are compared with the words
- **Then** a photo matches if its similarity reaches the model's cutoff, measured for each listed model
- **And** photos the chosen model hasn't looked at yet don't match, and the gallery's empty state says how many are still to come
- **And** vectors of another model are never compared with the words

### REQ-036-07 · Smart albums

- **Given** a search with a content chip
- **When** it is saved as a smart album (REQ-034-01)
- **Then** the album keeps the words, and opening it searches by content again with the model chosen then
- **And** its count in the sidebar follows the content matches

### REQ-036-08 · Changing models

- **Given** photos already looked at
- **When** the user chooses another model
- **Then** the photos are looked at again in the background with it, and going back to an earlier model doesn't repeat its work while its data is kept

## Out of scope

- "Find similar photos" from one photo, and searching with an image.
- Ranking by similarity instead of the gallery's sort order.
- Languages other than English (a multilingual model).
- Models the user adds from a file: a content model is three files whose tokenizer and input the app must know.
- A graphics card: models run on the CPU, as the face models do.
- The browser version.

## Assumptions

- A cached thumbnail (400 px wide) shows as much as the 224–256 px a model reads.
- The user accepts the license of a model they download; the section states it.
- Similarity is a hint: some matches are wrong and some photos are missed.

## Verification

| REQ | Test | Result |
| --- | --- | --- |
| REQ-036-01 | `crates/domain/src/content.rs` › req_036_01_lists_complete_models; `crates/app/tests/content.rs` › req_036_01_req_036_03_lists_the_models_and_starts_off, req_036_01_req_036_03_req_036_08_uses_a_model_and_deletes_the_data | pass |
| REQ-036-02 | `crates/media/src/content.rs` › req_036_02_keeps_a_model_only_if_every_hash_matches | pass (the real download over HTTPS checked by hand) |
| REQ-036-03 | `crates/indexer/src/content.rs` › req_036_03_looks_at_nothing_without_a_model; `crates/app/src/state/content.rs` › req_036_03_req_036_04_says_what_the_pass_is_doing; `crates/app/tests/content.rs` › req_036_01_req_036_03_lists_the_models_and_starts_off, req_036_01_req_036_03_req_036_08_uses_a_model_and_deletes_the_data, req_036_05_searches_by_what_the_photos_show | pass |
| REQ-036-04 | `crates/domain/src/content.rs` › req_036_04_crops_the_center_square; `crates/db/src/content.rs` › req_036_04_photos_and_videos_wait_until_looked_at_in_their_version, req_036_04_an_offline_library_waits_only_with_thumbnails; `crates/indexer/src/content.rs` › req_036_04_works_through_photos_and_videos_newest_first, req_036_04_req_036_08_pauses_and_follows_the_chosen_model, req_036_04_a_model_that_cant_run_says_so, req_036_04_reads_the_cached_thumbnail_first; `crates/app/src/state/content.rs` › req_036_03_req_036_04_says_what_the_pass_is_doing; `crates/media/src/content.rs` › req_036_04_req_036_05_a_real_model_matches_words_and_photos | pass (the real model only where `GG_CONTENT_MODELS` is set) |
| REQ-036-05 | `crates/domain/src/content.rs` › req_036_05_tokenizes_as_clip_does_and_pads_with_zero, req_036_05_maps_bytes_as_byte_level_bpe; `crates/domain/src/search.rs` › req_036_05_content_words_become_the_items_they_match; `crates/app/tests/content.rs` › req_036_05_searches_by_what_the_photos_show; `crates/app/src/state/content.rs` › req_036_05_req_036_06_a_real_model_finds_the_photo | pass (the real model only where `GG_CONTENT_MODELS` is set) |
| REQ-036-06 | `crates/domain/src/content.rs` › req_036_06_compares_stored_vectors_with_text; `crates/db/src/content.rs` › req_036_06_req_036_08_reads_back_each_models_vectors; `crates/app/tests/content.rs` › req_036_06_says_when_nothing_shows_it; `crates/app/src/state/content.rs` › req_036_05_req_036_06_a_real_model_finds_the_photo | partial: the cutoffs come from 12 photos (plan task 9) |
| REQ-036-07 | `crates/domain/src/smart_albums.rs` › req_036_07_keeps_the_content_words; `crates/db/src/smart_albums.rs` › req_036_07_counts_an_albums_content_matches; `crates/app/tests/content.rs` › req_036_07_a_smart_album_keeps_the_words | pass |
| REQ-036-08 | `crates/indexer/src/content.rs` › req_036_04_req_036_08_pauses_and_follows_the_chosen_model; `crates/db/src/content.rs` › req_036_06_req_036_08_reads_back_each_models_vectors; `crates/app/tests/content.rs` › req_036_01_req_036_03_req_036_08_uses_a_model_and_deletes_the_data | pass |
