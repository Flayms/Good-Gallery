# 031 · Faces

**Status:** Approved

## Goal

Users with large libraries want to find people without tagging each photo by hand. Good Gallery finds faces in photos on the user's own computer and suggests who they are. A name the user confirms goes into the file as a face region, so it survives a rebuilt index and shows in other programs. Which models do this is the user's choice (#36).

## Requirements

### REQ-031-01 · Choosing models

- **Given** the _Faces_ section of the settings
- **When** the user opens it
- **Then** it lists the detection and the recognition models with size and license, and links to the page comparing them (`docs/face-models.md` on GitHub), which opens in the default browser
- **And** the user picks one model of each kind among those on this computer

### REQ-031-02 · Downloading a listed model

- **Given** a listed model that isn't on this computer
- **When** the user chooses _Download_
- **Then** it downloads in the background with visible progress, without slowing browsing, and can be cancelled
- **And** once its SHA-256 matches the one built into the app it is available, also offline
- **And** a failed, cancelled or mismatching download leaves nothing behind, and the app says why

### REQ-031-03 · Adding another model

- **Given** an ONNX model that isn't listed
- **When** the user pastes an `https` link or picks a file, says whether it detects or recognizes, and for recognition chooses its input style (_ArcFace style_ or _SFace style_)
- **Then** the model is copied into the data folder and available like a listed one, under the file's name
- **And** a file the app can't run as that kind of model is refused with the reason, and leaves nothing behind

### REQ-031-04 · Off until chosen

- **Given** no detection model chosen
- **When** the app runs
- **Then** it looks for no faces and stores no face data
- **And** _Delete face data_ in the settings removes everything the app stored about faces, and never touches files

### REQ-031-05 · Finding faces in the background

- **Given** chosen models and photos not looked at yet with the chosen detection model
- **When** the app is running
- **Then** it works through them one by one on its own thread, stops when a share is offline or the setting is switched off, and carries on after a restart
- **And** a photo that changes is looked at again, and a file that can't be read costs only that file
- **And** the settings show how many photos are done
- **And** videos are skipped

### REQ-031-06 · Detecting well

- **Given** a photo of any size and orientation
- **When** faces are looked for
- **Then** a face is found whether it fills the photo or is small in a group, the same face isn't reported twice, and faces too small to recognize aren't kept
- **And** boxes and landmarks are relative to the upright photo

### REQ-031-07 · Recognizing well

- **Given** a detected face
- **When** it is recognized
- **Then** it is aligned by its five landmarks and prepared as its model expects, and the result is normalized
- **And** results of different recognition models are never compared with each other

### REQ-031-08 · Face regions of other programs

- **Given** a photo with named face regions (REQ-004-13), written by any program
- **When** faces are found in it
- **Then** a detected face overlapping a region counts as that region's person, with no second box, and teaches the app what the person looks like
- **And** their names and places aren't changed, unless the user changes them
- **And** when another program changes the regions, the file wins

### REQ-031-09 · Unknown faces in the viewer

- **Given** a photo with detected faces in the viewer, with face regions turned on (REQ-009-17)
- **When** a face has no name
- **Then** it is drawn as a box without a name, with its suggestion if there is one
- **And** the _Faces_ button also shows for such photos
- **And** clicking it offers: type a name, confirm the suggestion, _Not this person_, _Ignore_ and _Not a face_

### REQ-031-10 · Suggestions

- **Given** faces with a known person and a face without one, recognized by the same model
- **When** they are similar enough
- **Then** the person is suggested, and never assigned without the user
- **And** a person the user said this isn't is not suggested again for that face

### REQ-031-11 · Ignoring and removing detections

- **Given** a detected face without a name
- **When** the user chooses _Ignore_ or _Not a face_
- **Then** the box and any suggestion are gone, and don't come back when the photo is looked at again
- **And** an ignored face no longer counts in the groups (REQ-031-15), and can be brought back in the settings by _Show ignored faces again_

### REQ-031-12 · Drawing a face

- **Given** a photo in the viewer with face regions on
- **When** the user chooses _Add face_, draws a rectangle and types a name
- **Then** it is saved as a named face region

### REQ-031-13 · Removing or renaming a face region

- **Given** a named face region, written by any program
- **When** the user removes or renames it, on the photo or by its People tag's remove button
- **Then** the file changes accordingly, and its other regions and metadata stay
- **And** after removing, the same name isn't suggested for that face again

### REQ-031-14 · What reaches the file

- **Given** faces in a photo
- **When** the user confirms a name, draws, renames or removes a region
- **Then** the region goes into the file or its sidecar as ratings and tags do (spec 024, spec 026): the MWG region and `PersonInImage`, the Microsoft region only if the file already has that field, on the writer thread, and read back by the index
- **And** unknown faces, suggestions, rejections, ignored faces and what recognition computes stay out of the file
- **And** regions other than faces and fields the app doesn't know survive
- **And** a file that can't be written shows why, and its face stays unknown

### REQ-031-15 · Groups of unknown faces

- **Given** unknown faces of the chosen recognition model
- **When** the user opens _People_ from the sidebar's People group
- **Then** similar faces form groups, largest first, without groups of fewer than three
- **And** the user can name a group, which asks first and then writes to all its photos, _Ignore_ it, or take a wrong face out of it

### REQ-031-16 · Changing models

- **Given** faces already found
- **When** the user chooses another recognition model
- **Then** faces are recognized again in the background, names are untouched, and going back to an earlier model doesn't repeat its work
- **And** with another detection model photos are looked at again, and what the user ignored or rejected stays where the new box overlaps it

### REQ-031-17 · A model set up wrongly

- **Given** a recognition model and at least two people with several known faces
- **When** they aren't told apart better than chance
- **Then** the _Faces_ section warns that the model may be set up wrongly

## Out of scope

- Videos, pets and the browser version.
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
