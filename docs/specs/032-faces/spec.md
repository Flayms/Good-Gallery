# 032 · Faces

**Status:** Implemented

## Goal

Users with large libraries want to find people without tagging each photo by hand. Good Gallery finds faces in photos on the user's own computer and suggests who they are. A name the user confirms goes into the file as a face region, so it survives a rebuilt index and shows in other programs. Which models do this is the user's choice (#36).

## Requirements

### REQ-032-01 · Choosing models

- **Given** the _Faces_ section of the settings
- **When** the user opens it
- **Then** it lists the detection and the recognition models with size and license, and links to the page comparing them (`docs/face-models.md` on GitHub), which opens in the default browser
- **And** the user picks one model of each kind among those on this computer

### REQ-032-02 · Downloading a listed model

- **Given** a listed model that isn't on this computer
- **When** the user chooses _Download_
- **Then** it downloads in the background with visible progress, without slowing browsing, and can be cancelled
- **And** once its SHA-256 matches the one built into the app it is available, also offline
- **And** a failed, cancelled or mismatching download leaves nothing behind, and the app says why

### REQ-032-03 · Adding another model

- **Given** an ONNX model that isn't listed
- **When** the user picks a file or drops one on the _Faces_ section of the settings
- **Then** the app works out by itself whether it finds or recognizes faces, the model is copied into the data folder and it is available like a listed one, under the file's name
- **And** the one thing the app can't work out, how a recognition model expects its pixels, is a choice that says what it means, set to what most models use
- **And** a file the app can't run as that kind of model is refused with the reason, and leaves nothing behind
- **And** while a file is over the window the section shows where to drop it; a dropped file that isn't `.onnx` is refused with the reason, of several the first is added and a toast says so, and on any other page a dropped file does nothing

### REQ-032-04 · Off until chosen

- **Given** no detection model chosen
- **When** the app runs
- **Then** it looks for no faces and stores no face data
- **And** _Delete face data_ in the settings removes everything the app stored about faces, and never touches files

### REQ-032-05 · Finding faces in the background

- **Given** chosen models and photos not looked at yet with the chosen detection model
- **When** the app is running
- **Then** it works through them one by one on its own thread, stops when a share is offline or the setting is switched off, and carries on after a restart
- **And** a photo that changes is looked at again, and a file that can't be read costs only that file
- **And** the settings show how many photos are done
- **And** videos are skipped

### REQ-032-06 · Detecting well

- **Given** a photo of any size and orientation
- **When** faces are looked for
- **Then** a face is found whether it fills the photo or is small in a group, the same face isn't reported twice, and faces too small to recognize aren't kept
- **And** boxes and landmarks are relative to the upright photo

### REQ-032-07 · Recognizing well

- **Given** a detected face
- **When** it is recognized
- **Then** it is aligned by its five landmarks and prepared as its model expects, and the result is normalized
- **And** results of different recognition models are never compared with each other

### REQ-032-08 · Face regions of other programs

- **Given** a photo with named face regions (REQ-004-13), written by any program
- **When** faces are found in it
- **Then** a detected face overlapping a region counts as that region's person, with no second box, and teaches the app what the person looks like
- **And** their names and places aren't changed, unless the user changes them
- **And** when another program changes the regions, the file wins

### REQ-032-09 · Unknown faces in the viewer

- **Given** a photo with detected faces in the viewer, with face regions turned on (REQ-009-17)
- **When** a face has no name
- **Then** it is drawn as a box without a name, with its suggestion if there is one
- **And** the _Faces_ button also shows for such photos
- **And** clicking it offers: type a name, confirm the suggestion, _Not this person_, _Ignore_, _Not a face_ and _Remove box_ (REQ-032-33)

### REQ-032-10 · Suggestions

- **Given** faces with a known person and a face without one, recognized by the same model
- **When** they are similar enough
- **Then** the person is suggested, and never assigned without the user
- **And** a person the user said this isn't is not suggested again for that face

### REQ-032-11 · Ignoring and removing detections

- **Given** a detected face without a name
- **When** the user chooses _Ignore_ or _Not a face_
- **Then** the box and any suggestion are gone, and don't come back when the photo is looked at again
- **And** neither counts in the groups (REQ-032-15) or as a face to confirm (REQ-032-24); _Unconfirmed faces_ lists them to change the decision (REQ-032-30)

### REQ-032-12 · Drawing a face

- **Given** a photo in the viewer with face regions on
- **When** the user chooses _Add face_, draws a rectangle and types a name
- **Then** it is saved as a named face region

### REQ-032-13 · Removing or renaming a face region

- **Given** a named face region, written by any program
- **When** the user removes it (_Remove name_) or renames it, on the photo or by its People tag's remove button
- **Then** the file changes accordingly, and its other regions and metadata stay
- **And** after removing, the same name isn't suggested for that face again

### REQ-032-14 · What reaches the file

- **Given** faces in a photo
- **When** the user confirms a name, draws, renames or removes a region
- **Then** the region goes into the file or its sidecar as ratings and tags do (spec 024, spec 026): the MWG region and `PersonInImage`, the Microsoft region only if the file already has that field, on the writer thread, and read back by the index
- **And** unknown faces, suggestions, rejections, ignored faces and what recognition computes stay out of the file
- **And** regions other than faces and fields the app doesn't know survive
- **And** a file that can't be written shows why, and its face stays unknown

### REQ-032-15 · Groups of unknown faces

- **Given** unknown faces of the chosen recognition model, without a suggestion (REQ-032-24)
- **When** the user opens _Unconfirmed faces_ from the sidebar's People group
- **Then** similar faces form groups, largest first, without groups of fewer than three, below the faces to confirm
- **And** each face in a group on screen shows as a crop of its photo, soon after and while the page stays open; a photo without a thumbnail shows an empty square
- **And** the user can name a group, which asks first and then writes to all its photos, _Ignore_ it, mark it as _Not faces_, or take a wrong face out of it

### REQ-032-16 · Changing models

- **Given** faces already found
- **When** the user chooses another recognition model
- **Then** faces are recognized again in the background, names are untouched, and going back to an earlier model doesn't repeat its work
- **And** with another detection model photos are looked at again, and what the user ignored or rejected stays where the new box overlaps it

### REQ-032-17 · A model set up wrongly

- **Given** a recognition model and at least two people with several known faces
- **When** they aren't told apart better than chance
- **Then** the _Faces_ section warns that the model may be set up wrongly

### REQ-032-18 · Recommended models

- **Given** the models with a download button in the _Faces_ section
- **When** the user looks at them
- **Then** one finding and one recognizing model each are marked as recommended for speed and for accuracy, and each says why in words, based on the measurements in `docs/face-models.md`, without the scores themselves
- **And** a model with a button that isn't recommended says what it is there for
- **And** no model labeled for research only has a button

### REQ-032-19 · What choosing a model does

- **Given** the _Faces_ section
- **When** the user reads it, before choosing anything
- **Then** it says that choosing a finding model starts looking through the photos in the background, what a recognizing model adds, and that changing a model looks again while confirmed names stay
- **And** the model in use reads _In use_, and clicking it changes nothing: stopping is _Pause_, not unchoosing

### REQ-032-20 · What the search is doing

- **Given** a chosen finding model
- **When** the user opens the _Faces_ section
- **Then** its first line says what is going on: finding faces, finding and recognizing them, only recognizing, paused, waiting for an offline library, finished or stopped by a problem, with how far it has come, and offers _Pause_ or _Resume_
- **And** while photos are waiting it counts photos and the faces found, for both steps when both models are chosen (REQ-032-23): `1,234 of 50,000 photos done, 321 faces found`; only recognizing, it counts faces: `120 of 3,210 faces recognized`
- **And** the real number of photos shows from the start, before the first photo is done, and it never says `0 of 0`
- **And** pausing stops after the current photo and keeps everything found, and faces and suggestions stay visible
- **And** while it works, the processing icon shows on every page; its menu says what the first line says and leads to the _Faces_ section (#175)

### REQ-032-21 · Which photos were looked at

- **Given** a photo with a chosen finding model
- **When** its details are shown
- **Then** they say whether it was looked at yet and how many faces were found

### REQ-032-22 · Looking again

- **Given** a photo that was looked at
- **When** the user chooses _Look again_ in its details, or _Look through all photos again_ in the settings, which asks first
- **Then** those photos are looked at again in the background
- **And** what the user decided about a face stays (REQ-032-11), and names are untouched

### REQ-032-23 · Recognized right after being found

- **Given** a chosen detection and recognition model, and photos not looked at yet
- **When** the search works through them
- **Then** each photo's faces are recognized right after it was looked at, before the next photo, so suggestions appear during the first pass, and the photo is read and decoded once for both steps
- **And** faces waiting for a recognition model chosen later are recognized once no photos are waiting (REQ-032-16)

### REQ-032-24 · Recognized faces to confirm

- **Given** unknown faces of the chosen recognition model with a suggestion (REQ-032-10)
- **When** the user opens _Unconfirmed faces_
- **Then** they come first, one group per suggested person, largest first, titled with the name as a question ("Anna? · 14 faces")
- **And** _Yes, all_ asks first and then writes the name to all of the group's photos, as naming a group does (REQ-032-15)
- **And** a face's ✕ says _Not Anna_: that person is no longer suggested for it (REQ-032-10), and it joins the unknown faces or another person's group

### REQ-032-25 · Taking a face out of a group

- **Given** a group of unknown faces
- **When** the user chooses a face's ✕ (_Not one of them_)
- **Then** the face leaves the groups for good, also when they are formed again
- **And** it stays an unknown face: the viewer still shows it with its suggestion, it still counts as a face to confirm (REQ-032-24), and without a suggestion it shows under _Other unknown faces_ (REQ-032-28)
- **And** _Ignore_ and _Not faces_ for the whole group stay as they were (REQ-032-11)

### REQ-032-26 · The whole photo

- **Given** a face on _Unconfirmed faces_
- **When** the user clicks it
- **Then** the viewer opens on its photo, with face regions shown (REQ-009-17), and the filmstrip, ← and → go through that group's photos only
- **And** the faces can be named or rejected there as in any photo (REQ-032-09)
- **And** closing the viewer returns to _Unconfirmed faces_, with the gallery's search as it was

### REQ-032-27 · Unconfirmed faces in the People group

- **Given** faces are recognized (the People page has something to show)
- **When** the sidebar is shown
- **Then** _Unconfirmed faces_ is the first row of the People group, below its heading, above the people's tags
- **And** collapsing the People group hides it with the tags, and the group shows with its heading even without People tags

### REQ-032-28 · Every unknown face on the page

- **Given** unknown faces of the chosen recognition model without a suggestion, in no group (taken out of one, REQ-032-25, or with fewer than three alike)
- **When** the user opens _Unconfirmed faces_
- **Then** they show under _Other unknown faces_, after the groups, a first part of them with _Show more_ for the rest
- **And** a face there opens its photo among theirs, as a group's face does (REQ-032-26)

### REQ-032-29 · Deciding on the page

- **Given** a face on _Unconfirmed faces_ that the user hasn't decided about
- **When** the user opens its menu (right click)
- **Then** it offers _Ignore_, _Not a face_ and _Remove box_, with the same effect as in the viewer (REQ-032-11, REQ-032-33)

### REQ-032-30 · Ignored faces and non-faces

- **Given** faces the user ignored or marked as not a face, of the chosen detection model
- **When** the user opens _Unconfirmed faces_
- **Then** they are listed at the end, under _Ignored_ and _Not faces_, each with its count, collapsed until opened, also when no face is left to confirm
- **And** clicking a face offers: type a name, which writes it as a face region (REQ-032-14), _Back to unknown_, the other decision, and _Remove box_
- **And** _Remove box_ forgets the face; it is found again only if its photo is looked at again (REQ-032-22)
- **And** the settings have no button bringing back all ignored faces

### REQ-032-31 · Deciding without waiting

- **Given** _Unconfirmed faces_ with faces on it
- **When** the user decides about faces there (names, ignores, marks as not faces, takes out of a group, removes)
- **Then** the faces decided about leave their group or section at once
- **And** the rest of the page stays as it was while the groups are formed again, without a message that the faces are being looked through

### REQ-032-32 · Every face of a group

- **Given** a group of faces on _Unconfirmed faces_, to confirm (REQ-032-24) or unknown (REQ-032-15)
- **When** the page shows it
- **Then** all of its faces show, a first part of a large group with _Show more_ for the rest, as under _Other unknown faces_ (REQ-032-28)

### REQ-032-33 · What each decision means

- **Given** a face in the viewer or on _Unconfirmed faces_
- **When** the user points at one of its decisions
- **Then** a short tip says what it does and when to use it: naming writes the name into the photo; _Ignore_ is for a real person the user won't name; _Not a face_ is for a wrong detection, which isn't found again; _Remove box_ forgets the detection, which may be found again when the photo is looked at again; _Remove name_ takes the name out of the photo
- **And** a button that names or renames is off until a name is typed, and _Rename_ until it differs
- **And** the same decisions have the same words everywhere (`docs/glossary.md`)

### REQ-032-34 · Known people while typing a name

- **Given** a field for a face's name, in the viewer or on _Unconfirmed faces_
- **When** the user types in it
- **Then** up to six known people (People tags) whose name contains what was typed show below it, those starting with it first
- **And** clicking one, or moving to it with ↓ and ↑ and pressing Enter, names the face or group as typing the name and pressing Enter would

## Out of scope

- Videos, pets and the browser version.
- Changing several decided faces at once.
- Merging two people, and a page of confirmed people: the sidebar's People group lists them.
- Showing unnamed face regions of other programs: they stay in the file, and the detector usually finds the same faces.
- A graphics card: models run on the CPU (see the plan).
- Models in other formats than ONNX, and detection models of an output format the app doesn't know.
- Keeping rejections and ignored faces when the index is rebuilt. They live in the index, like everything not written to files.

## Assumptions

- The user is responsible for the license of a model they add. `docs/face-models.md` says what is known about each.
- Face similarity is a hint. Suggestions are wrong sometimes, so nothing reaches a file without the user.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-032-01 | `crates/app/tests/faces.rs` › req_032_01_lists_the_models_with_their_licenses, req_032_19_says_what_choosing_does_and_in_use_stays, req_032_01_settings_section_looks_as_designed (snapshot `settings_faces`) | pass |
| REQ-032-02 | `crates/domain/src/face_models.rs` › req_032_02_the_catalog_pins_what_it_downloads; `crates/media/src/face_store.rs` › req_032_02_keeps_a_download_only_if_its_hash_matches, req_032_02_a_cancelled_or_failed_download_leaves_nothing, req_032_02_downloads_a_small_real_model_over_https (ignored in CI, run by hand) | partial: progress and cancelling are covered in `media`, not on the page |
| REQ-032-03 | `crates/media/src/face_models.rs` › req_032_03_refuses_a_file_that_is_no_model_or_the_wrong_kind; `crates/media/src/face_models.rs` › req_032_03_works_out_what_a_model_is; `crates/media/src/face_store.rs` › req_032_03_adds_a_file_only_if_it_runs; `crates/app/tests/faces.rs` › req_032_03_adds_a_file_it_can_run_and_refuses_one_it_cant, req_032_03_has_no_link_field_and_says_a_file_can_be_dropped, req_032_03_adds_a_dropped_file_and_refuses_one_it_cant_run, req_032_03_says_why_a_dropped_file_isnt_added, req_032_03_a_file_dropped_on_another_page_is_ignored, req_032_03_shows_where_to_drop_a_file | pass |
| REQ-032-04 | `crates/indexer/src/faces.rs` › req_032_04_looks_for_nothing_without_a_chosen_model; `crates/db/src/faces.rs` › req_032_04_deletes_all_face_data_but_no_named_regions; `crates/app/tests/faces.rs` › req_032_04_delete_face_data_asks_and_stops_the_search | pass |
| REQ-032-05 | `crates/db/src/faces.rs` › req_032_05_works_through_photos_once_and_again_when_they_change, req_032_05_skips_offline_libraries; `crates/indexer/src/faces.rs` › req_032_05_works_through_all_photos_and_a_bad_file_costs_only_itself, req_032_05_stops_for_an_unreachable_share_or_when_paused, req_032_05_a_model_that_cant_run_says_so_and_a_panic_costs_only_that_file | pass |
| REQ-032-06 | `crates/domain/src/face_models.rs` › req_032_06_finds_the_same_face_only_once, req_032_06_decodes_yunets_output, req_032_06_decodes_retinafaces_output, req_032_06_chooses_passes_by_what_the_first_found, req_032_06_turns_what_a_pass_found_into_photo_pixels_and_drops_tiny_faces; `crates/media/src/face_models.rs` › req_032_06_finds_a_real_face_with_its_landmarks, req_032_06_finds_small_faces_on_a_large_photo, req_032_06_finds_a_face_that_fills_the_photo, req_032_06_retinaface_finds_a_real_face (ignored in CI, run by hand); `crates/indexer/src/faces.rs` › req_032_06_finds_the_face_of_a_turned_photo_with_the_real_model, req_032_06_detections_become_fractions_of_the_upright_photo | pass, with rates measured on WIDER FACE (`docs/face-models.md`), not on a real library |
| REQ-032-07 | `crates/domain/src/face_models.rs` › req_032_07_aligns_a_turned_face_onto_the_template, req_032_07_prepares_pixels_as_the_style_expects, req_032_07_normalizes_and_compares_only_the_same_size; `crates/media/src/face_models.rs` › req_032_07_moves_the_landmarks_onto_the_template_in_the_crop, req_032_07_gives_a_normalized_vector_prepared_for_its_style, req_032_07_sface_tells_people_apart, req_032_07_auraface_tells_people_apart (both ignored in CI, run by hand: same person 0.97 and 0.95, others about 0); `crates/db/src/faces.rs` › req_032_07_keeps_each_models_vectors_apart | pass, with rates measured on LFW (`docs/face-models.md`), not on a real library |
| REQ-032-08 | `crates/domain/src/face_models.rs` › req_032_08_a_loose_region_of_another_program_is_the_same_face; `crates/db/src/faces.rs` › req_032_08_a_face_on_a_named_region_is_that_person_until_the_file_changes | pass |
| REQ-032-09 | `crates/app/tests/faces.rs` › req_032_09_shows_unknown_faces_with_suggestions_beside_named_ones, req_032_09_faces_button_shows_when_faces_are_looked_for, req_032_09_no_faces_button_while_none_are_looked_for, req_032_09_naming_an_unknown_face_writes_the_typed_name, req_032_09_a_faces_popup_looks_as_designed (snapshot `viewer_face_popup`) | pass |
| REQ-032-10 | `crates/domain/src/face_models.rs` › req_032_10_suggests_the_most_similar_person_unless_rejected, req_032_10_one_wrong_face_of_a_person_counts_for_little, req_032_10_thresholds_are_the_measured_ones; `crates/media/src/face_store.rs` › req_032_10_the_catalog_beats_an_old_note; `crates/db/src/faces.rs` › req_032_10_remembers_who_a_face_is_not, req_032_10_suggests_a_known_person_for_an_unknown_face; `crates/app/tests/faces.rs` › req_032_10_not_this_person_is_remembered | pass, with thresholds measured on LFW, not on a real library |
| REQ-032-11 | `crates/db/src/faces.rs` › req_032_11_ignored_faces_and_non_faces_stay_gone_when_looked_at_again; `crates/app/tests/faces.rs` › req_032_11_ignored_and_non_faces_go_away, req_032_11_a_non_face_goes_away | pass |
| REQ-032-12 | `crates/media/src/face_regions.rs` › req_032_12_a_new_region_goes_into_the_file_and_reads_back_where_it_was_drawn; `crates/app/tests/faces.rs` › req_032_12_draws_a_face_and_saves_it_where_it_was_drawn | pass |
| REQ-032-13 | `crates/media/src/face_regions.rs` › req_032_13_removes_and_renames_one_region_of_any_program, req_032_13_a_person_with_another_region_stays_in_person_in_image; `crates/app/tests/faces.rs` › req_032_13_renames_and_removes_a_region_on_the_photo, req_032_13_removing_on_the_photo_takes_the_region_and_the_keyword; `crates/app/tests/tagging.rs` › req_032_13_the_people_tags_remove_button_takes_the_region | pass |
| REQ-032-14 | `crates/domain/src/faces.rs` › req_032_14_stored_rect_undoes_turning_upright; `crates/media/src/face_regions.rs` › req_032_14_other_regions_and_fields_survive, req_032_14_changes_the_microsoft_region_only_where_it_exists, req_032_14_stores_rectangles_for_the_files_orientation, req_032_14_uses_the_sidecar_when_only_it_has_regions, req_032_14_a_read_only_file_shows_why_and_stays, req_032_14_is_read_back_by_the_indexing_rules; `crates/app/tests/faces.rs` › req_032_14_confirming_a_suggestion_writes_it_and_nothing_before_that, req_032_14_rejecting_and_ignoring_leave_the_file_alone | pass |
| REQ-032-15 | `crates/domain/src/face_models.rs` › req_032_15_groups_similar_faces_largest_first_without_small_groups; `crates/db/src/faces.rs` › req_032_15_groups_only_faces_without_a_name; `crates/app/src/people_page.rs` › req_032_15_crops_faces_without_stretching_them; `crates/app/tests/faces.rs` › req_032_15_names_a_group_after_asking_and_writes_to_all_its_photos, req_032_15_ignoring_a_group_or_taking_a_face_out_leaves_too_few, req_032_15_no_people_page_without_models, req_032_15_shows_the_faces_of_a_group, req_032_15_loads_only_the_faces_of_groups_near_the_screen, req_032_15_people_page_looks_as_designed (snapshot `people_page`) | pass |
| REQ-032-16 | `crates/indexer/src/faces.rs` › req_032_16_another_recognition_model_recognizes_again_and_going_back_repeats_nothing; `crates/db/src/faces.rs` › req_032_16_decisions_carry_over_to_another_detector_and_earlier_work_stays | pass |
| REQ-032-18 | `crates/domain/src/face_models.rs` › req_032_18_recommends_one_for_speed_and_one_for_accuracy, req_032_18_descriptions_have_no_scores_and_no_second_link; `crates/app/tests/faces.rs` › req_032_18_says_why_a_model_is_recommended | pass |
| REQ-032-19 | `crates/app/tests/faces.rs` › req_032_19_says_what_choosing_does_and_in_use_stays | pass |
| REQ-032-20 | `crates/app/src/state/faces.rs` › req_032_20_says_what_the_search_is_doing, req_032_20_never_says_none_of_none, req_032_20_a_notice_only_while_it_works; `crates/indexer/src/faces.rs` › req_032_20_counts_the_photos_before_the_first_is_done; `crates/app/tests/faces.rs` › req_032_20_pause_and_resume, req_032_20_the_icon_shows_on_every_page_and_leads_to_the_settings | pass |
| REQ-032-21 | `crates/db/src/faces.rs` › req_032_21_says_whether_a_photo_was_looked_at; `crates/app/tests/faces.rs` › req_032_21_details_say_whether_a_photo_was_looked_at | pass |
| REQ-032-22 | `crates/db/src/faces.rs` › req_032_22_looks_again_and_decisions_stay; `crates/app/tests/faces.rs` › req_032_22_look_again_from_the_details, req_032_22_look_through_all_again_asks_first | pass |
| REQ-032-23 | `crates/indexer/src/faces.rs` › req_032_23_recognizes_a_photos_faces_right_after_looking_at_it, req_032_23_only_looks_without_a_recognition_model_and_catches_up_with_one; `crates/db/src/faces.rs` › req_032_23_only_the_open_faces_of_that_photo | pass |
| REQ-032-17 | `crates/domain/src/face_models.rs` › req_032_17_notices_a_model_that_tells_nobody_apart; `crates/db/src/faces.rs` › req_032_17_a_model_telling_people_apart_seems_right; `crates/app/tests/faces.rs` › req_032_17_warns_when_the_model_tells_nobody_apart | pass |
| REQ-032-24 | `crates/db/src/faces.rs` › req_032_24_groups_suggested_faces_by_person; `crates/app/tests/faces.rs` › req_032_24_confirm_or_reject_a_suggested_group | pass |
| REQ-032-25 | `crates/db/src/faces.rs` › req_032_25_a_face_taken_out_stays_unknown; `crates/app/tests/faces.rs` › req_032_25_taking_a_face_out_keeps_it_in_the_viewer | pass |
| REQ-032-26 | `crates/db/src/gallery.rs` › req_032_26_a_search_of_only_some_items; `crates/app/tests/faces.rs` › req_032_26_a_face_opens_its_photo_among_its_groups, req_032_26_the_viewer_from_the_page_has_its_thumbnails | pass |
| REQ-032-28 | `crates/db/src/faces.rs` › req_032_28_faces_in_no_group_are_the_other_unknown_faces; `crates/app/tests/faces.rs` › req_032_28_faces_in_no_group_show_and_open_among_theirs, req_032_28_shows_more_on_asking, req_032_30_decided_faces_look_as_designed (snapshot `people_page_decided`) | pass |
| REQ-032-29 | `crates/app/tests/faces.rs` › req_032_29_ignore_or_not_a_face_from_the_page; snapshot `people_page` | pass |
| REQ-032-30 | `crates/db/src/faces.rs` › req_032_30_lists_decided_faces_to_undo_swap_or_delete; `crates/app/tests/faces.rs` › req_032_30_change_or_delete_a_decision, req_032_30_naming_a_decided_face_writes_it, req_032_30_no_button_brings_back_all_ignored_faces, req_032_30_decided_faces_look_as_designed (snapshot `people_page_decided`) | pass |
| REQ-032-27 | `crates/app/tests/faces.rs` › req_032_27_unconfirmed_faces_is_the_first_row_of_people, req_032_27_people_shows_without_tags_and_collapses_with_it; snapshot `people_page` | pass |
| REQ-032-31 | `crates/app/tests/faces.rs` › req_032_31_deciding_keeps_the_page_while_it_loads_again | pass |
| REQ-032-32 | `crates/app/tests/faces.rs` › req_032_32_a_group_shows_all_its_faces | pass |
| REQ-032-33 | `crates/app/tests/faces.rs` › req_032_33_the_viewer_says_what_each_decision_does, req_032_33_the_page_says_what_each_decision_does, req_032_13_renames_and_removes_a_region_on_the_photo (_Remove name_); snapshots `viewer_face_popup`, `people_page` | pass |
| REQ-032-34 | `crates/app/src/names.rs` › req_032_34_suggests_known_people_starting_with_what_was_typed_first; `crates/app/tests/faces.rs` › req_032_34_picks_a_known_person_on_the_page, req_032_34_picks_a_known_person_with_the_keys, req_032_34_suggests_known_people_in_the_viewer; snapshot `people_page_names` | pass |
