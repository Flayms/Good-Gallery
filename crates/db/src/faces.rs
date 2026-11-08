//! Faces found in photos (spec 032). Names live in the files and `media_faces`: a face overlapping a named region is
//! that person, worked out here when read, so a change by another program needs no syncing (REQ-032-08).

use crate::Result;
use domain::face_models::{Detection, group, same_face, suggestions, tells_nobody_apart};
use domain::faces::FaceRegion;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::PathBuf;

/// The setting holding the chosen detection model's hash.
pub const DETECTOR_KEY: &str = "faces.detector";
/// The setting holding the chosen recognition model's hash.
pub const RECOGNIZER_KEY: &str = "faces.recognizer";
/// How similar faces must be for the chosen recognition model, as text.
pub const THRESHOLD_KEY: &str = "faces.threshold";
/// Whether the search goes on, `"paused"` if the user switched it off.
pub const SEARCH_KEY: &str = "faces.search";

/// A face the user dealt with, or not yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceState {
    Open,
    Ignored,
    NotAFace,
}

impl FaceState {
    fn as_str(self) -> &'static str {
        match self {
            FaceState::Open => "open",
            FaceState::Ignored => "ignored",
            FaceState::NotAFace => "not_face",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "ignored" => FaceState::Ignored,
            "not_face" => FaceState::NotAFace,
            _ => FaceState::Open,
        }
    }
}

/// A photo's file, as face search needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct FacePhoto {
    pub media: i64,
    pub root_path: String,
    pub rel_path: String,
    pub size: i64,
    pub mtime: i64,
    pub sidecar_orientation: Option<u8>,
    pub root_id: i64,
}

impl FacePhoto {
    /// The file; `None` if its path would leave the library (REQ-011-08).
    pub fn file(&self) -> Option<PathBuf> {
        crate::media::absolute_path(&self.root_path, &self.rel_path)
    }
}

fn photo_from_row(row: &rusqlite::Row) -> rusqlite::Result<FacePhoto> {
    Ok(FacePhoto {
        media: row.get(0)?,
        root_path: row.get(1)?,
        rel_path: row.get(2)?,
        size: row.get(3)?,
        mtime: row.get(4)?,
        sidecar_orientation: row.get(5)?,
        root_id: row.get(6)?,
    })
}

const PHOTO_COLUMNS: &str = "m.id, r.path, m.rel_path, m.size, m.mtime, m.sidecar_orientation, m.root_id";

/// Photos `detector` hasn't looked at in their current version, newest first; videos and offline libraries don't count.
pub fn pending_photos(conn: &Connection, detector: &str, limit: usize) -> Result<Vec<FacePhoto>> {
    let sql = format!(
        "SELECT {PHOTO_COLUMNS} FROM media m JOIN library_roots r ON r.id = m.root_id
         LEFT JOIN faces_scanned s ON s.media_id = m.id AND s.detector = ?1
         WHERE m.kind = 'image' AND r.status <> 'offline'
           AND (s.media_id IS NULL OR s.size <> m.size OR s.mtime <> m.mtime)
         ORDER BY m.sort_date DESC LIMIT ?2"
    );
    let photos = conn
        .prepare_cached(&sql)?
        .query_map(params![detector, limit as i64], photo_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(photos)
}

fn landmarks_to_blob(landmarks: &[[f32; 2]; 5]) -> Vec<u8> {
    landmarks.iter().flatten().flat_map(|n| n.to_le_bytes()).collect()
}

fn landmarks_from_blob(blob: &[u8]) -> [[f32; 2]; 5] {
    let mut numbers = blob.chunks_exact(4).map(|bytes| {
        let bytes: [u8; 4] = bytes.try_into().unwrap_or_default();
        f32::from_le_bytes(bytes)
    });
    std::array::from_fn(|_| {
        let x = numbers.next().unwrap_or(0.0);
        [x, numbers.next().unwrap_or(0.0)]
    })
}

pub fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|n| n.to_le_bytes()).collect()
}

fn vector_from_blob(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap_or_default()))
        .collect()
}

/// Stores what `detector` found in `photo` (`faces` in fractions of the upright photo), replacing what it found before.
/// What the user decided about a box carries over to a new box overlapping it, also from another detector (REQ-032-11,
/// REQ-032-16).
pub fn store_scan(conn: &mut Connection, photo: &FacePhoto, detector: &str, faces: &[Detection]) -> Result<()> {
    let tx = conn.transaction()?;
    let earlier = {
        let mut stmt =
            tx.prepare_cached("SELECT f.id, f.x, f.y, f.w, f.h, f.state FROM detected_faces f WHERE f.media_id = ?1")?;
        let rows = stmt.query_map([photo.media], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                [row.get::<_, f32>(1)?, row.get(2)?, row.get(3)?, row.get(4)?],
                FaceState::parse(&row.get::<_, String>(5)?),
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut decided: Vec<([f32; 4], FaceState, Vec<String>)> = Vec::new();
    for (id, rect, state) in &earlier {
        let names: Vec<String> = tx
            .prepare_cached("SELECT name FROM face_rejections WHERE face_id = ?1")?
            .query_map([id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if *state != FaceState::Open || !names.is_empty() {
            decided.push((*rect, *state, names));
        }
    }
    tx.prepare_cached("DELETE FROM detected_faces WHERE media_id = ?1 AND detector = ?2")?
        .execute(params![photo.media, detector])?;
    for face in faces {
        let [x, y, w, h] = face.rect;
        let earlier = decided.iter().find(|(rect, _, _)| same_face(*rect, face.rect));
        let state = earlier.map_or(FaceState::Open, |(_, state, _)| *state);
        let id: i64 = tx
            .prepare_cached(
                "INSERT INTO detected_faces (media_id, detector, x, y, w, h, landmarks, score, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id",
            )?
            .query_row(
                params![
                    photo.media,
                    detector,
                    x,
                    y,
                    w,
                    h,
                    landmarks_to_blob(&face.landmarks),
                    face.score,
                    state.as_str()
                ],
                |row| row.get(0),
            )?;
        if let Some((_, _, names)) = earlier {
            for name in names {
                tx.prepare_cached("INSERT OR IGNORE INTO face_rejections (face_id, name) VALUES (?1, ?2)")?
                    .execute(params![id, name])?;
            }
        }
    }
    tx.prepare_cached(
        "INSERT OR REPLACE INTO faces_scanned (media_id, detector, size, mtime) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![photo.media, detector, photo.size, photo.mtime])?;
    tx.commit()?;
    Ok(())
}

/// A face waiting for a recognition model.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceToRecognize {
    pub face: i64,
    pub photo: FacePhoto,
    pub landmarks: [[f32; 2]; 5],
}

/// Faces of `detector` that `recognizer` hasn't seen, by photo, so a photo is decoded once. Ignored faces and non-faces
/// aren't worth the time. Skips offline libraries.
pub fn pending_faces(
    conn: &Connection,
    detector: &str,
    recognizer: &str,
    limit: usize,
) -> Result<Vec<FaceToRecognize>> {
    let sql = format!(
        "SELECT {PHOTO_COLUMNS}, f.id, f.landmarks FROM detected_faces f
         JOIN media m ON m.id = f.media_id JOIN library_roots r ON r.id = m.root_id
         LEFT JOIN face_embeddings e ON e.face_id = f.id AND e.recognizer = ?2
         WHERE f.detector = ?1 AND f.state = 'open' AND e.face_id IS NULL AND r.status <> 'offline'
         ORDER BY f.media_id LIMIT ?3"
    );
    let faces = conn
        .prepare_cached(&sql)?
        .query_map(params![detector, recognizer, limit as i64], face_to_recognize)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(faces)
}

/// The faces of one photo that `recognizer` hasn't seen, like [`pending_faces`] (REQ-032-23).
pub fn pending_faces_in(
    conn: &Connection,
    media: i64,
    detector: &str,
    recognizer: &str,
) -> Result<Vec<FaceToRecognize>> {
    let sql = format!(
        "SELECT {PHOTO_COLUMNS}, f.id, f.landmarks FROM detected_faces f
         JOIN media m ON m.id = f.media_id JOIN library_roots r ON r.id = m.root_id
         LEFT JOIN face_embeddings e ON e.face_id = f.id AND e.recognizer = ?3
         WHERE f.media_id = ?1 AND f.detector = ?2 AND f.state = 'open' AND e.face_id IS NULL"
    );
    let faces = conn
        .prepare_cached(&sql)?
        .query_map(params![media, detector, recognizer], face_to_recognize)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(faces)
}

fn face_to_recognize(row: &rusqlite::Row<'_>) -> rusqlite::Result<FaceToRecognize> {
    Ok(FaceToRecognize {
        photo: photo_from_row(row)?,
        face: row.get(7)?,
        landmarks: landmarks_from_blob(&row.get::<_, Vec<u8>>(8)?),
    })
}

/// Stores what `recognizer` made of `face`. An empty vector marks a face it can't read, so it isn't tried again.
pub fn store_embedding(conn: &Connection, face: i64, recognizer: &str, vector: &[f32]) -> Result<()> {
    conn.prepare_cached("INSERT OR REPLACE INTO face_embeddings (face_id, recognizer, vector) VALUES (?1, ?2, ?3)")?
        .execute(params![face, recognizer, vector_to_blob(vector)])?;
    Ok(())
}

/// How far the search has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FaceProgress {
    pub photos_done: u64,
    pub photos: u64,
    pub faces: u64,
    /// Of `faces`, those the chosen recognition model has seen.
    pub faces_recognized: u64,
}

pub fn progress(conn: &Connection, detector: &str, recognizer: Option<&str>) -> Result<FaceProgress> {
    let photos: i64 = conn
        .prepare_cached("SELECT count(*) FROM media WHERE kind = 'image'")?
        .query_row([], |row| row.get(0))?;
    let photos_done: i64 = conn
        .prepare_cached(
            "SELECT count(*) FROM faces_scanned s JOIN media m ON m.id = s.media_id
             WHERE s.detector = ?1 AND s.size = m.size AND s.mtime = m.mtime",
        )?
        .query_row([detector], |row| row.get(0))?;
    let faces: i64 = conn
        .prepare_cached("SELECT count(*) FROM detected_faces WHERE detector = ?1 AND state = 'open'")?
        .query_row([detector], |row| row.get(0))?;
    let faces_recognized: i64 = conn
        .prepare_cached(
            "SELECT count(*) FROM detected_faces f JOIN face_embeddings e ON e.face_id = f.id AND e.recognizer = ?2
             WHERE f.detector = ?1 AND f.state = 'open'",
        )?
        .query_row(params![detector, recognizer.unwrap_or_default()], |row| row.get(0))?;
    Ok(FaceProgress {
        faces_recognized: faces_recognized as u64,
        photos_done: photos_done as u64,
        photos: photos as u64,
        faces: faces as u64,
    })
}

/// A face on a photo in the viewer, without a name.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownFace {
    pub id: i64,
    pub media: i64,
    pub rect: [f32; 4],
    pub vector: Option<Vec<f32>>,
    pub rejected: Vec<String>,
    /// Taken out of the groups (REQ-032-25).
    pub ungrouped: bool,
}

fn regions(conn: &Connection, media: Option<i64>) -> Result<HashMap<i64, Vec<FaceRegion>>> {
    let (sql, media) = match media {
        Some(media) => (
            "SELECT media_id, name, x, y, w, h FROM media_faces WHERE media_id = ?1",
            media,
        ),
        None => ("SELECT media_id, name, x, y, w, h FROM media_faces WHERE ?1 = 0", 0),
    };
    let mut named: HashMap<i64, Vec<FaceRegion>> = HashMap::new();
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([media], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            FaceRegion {
                name: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
                w: row.get(4)?,
                h: row.get(5)?,
            },
        ))
    })?;
    for row in rows {
        let (media, region) = row?;
        named.entry(media).or_default().push(region);
    }
    Ok(named)
}

/// The named region a detected box lies on (REQ-032-08).
fn region_of(rect: [f32; 4], regions: Option<&Vec<FaceRegion>>) -> Option<&FaceRegion> {
    regions?
        .iter()
        .find(|region| same_face(rect, [region.x, region.y, region.w, region.h]))
}

fn rejections(conn: &Connection, face: i64) -> Result<Vec<String>> {
    Ok(conn
        .prepare_cached("SELECT name FROM face_rejections WHERE face_id = ?1 ORDER BY name")?
        .query_map([face], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn usable(blob: Vec<u8>) -> Option<Vec<f32>> {
    let vector = vector_from_blob(&blob);
    (!vector.is_empty()).then_some(vector)
}

/// Faces of `detector` on one photo without a name, with what `recognizer` made of them (REQ-032-09).
pub fn unknown_faces_on(conn: &Connection, media: i64, detector: &str, recognizer: &str) -> Result<Vec<UnknownFace>> {
    let named = regions(conn, Some(media))?;
    let mut stmt = conn.prepare_cached(
        "SELECT f.id, f.x, f.y, f.w, f.h, e.vector FROM detected_faces f
         LEFT JOIN face_embeddings e ON e.face_id = f.id AND e.recognizer = ?3
         WHERE f.media_id = ?1 AND f.detector = ?2 AND f.state = 'open' ORDER BY f.id",
    )?;
    let rows = stmt.query_map(params![media, detector, recognizer], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            [row.get::<_, f32>(1)?, row.get(2)?, row.get(3)?, row.get(4)?],
            row.get::<_, Option<Vec<u8>>>(5)?,
        ))
    })?;
    let mut faces = Vec::new();
    for row in rows {
        let (id, rect, vector) = row?;
        if region_of(rect, named.get(&media)).is_none() {
            faces.push(UnknownFace {
                id,
                media,
                rect,
                vector: vector.and_then(usable),
                rejected: rejections(conn, id)?,
                ungrouped: false,
            });
        }
    }
    Ok(faces)
}

/// Every face without a name that `recognizer` has seen, for the groups (REQ-032-15), with the people it isn't
/// (REQ-032-10). Faces of a person who was taken off a face stay.
pub fn unknown_faces(conn: &Connection, detector: &str, recognizer: &str) -> Result<Vec<UnknownFace>> {
    let named = regions(conn, None)?;
    let mut stmt = conn.prepare_cached(
        "SELECT f.id, f.media_id, f.x, f.y, f.w, f.h, e.vector, f.ungrouped FROM detected_faces f
         JOIN face_embeddings e ON e.face_id = f.id AND e.recognizer = ?2
         WHERE f.detector = ?1 AND f.state = 'open' ORDER BY f.id",
    )?;
    let rows = stmt.query_map(params![detector, recognizer], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            [row.get::<_, f32>(2)?, row.get(3)?, row.get(4)?, row.get(5)?],
            row.get::<_, Vec<u8>>(6)?,
            row.get::<_, bool>(7)?,
        ))
    })?;
    let mut rejected: HashMap<i64, Vec<String>> = HashMap::new();
    let mut names = conn.prepare_cached("SELECT face_id, name FROM face_rejections ORDER BY name")?;
    for row in names.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))? {
        let (face, name) = row?;
        rejected.entry(face).or_default().push(name);
    }
    let mut faces = Vec::new();
    for row in rows {
        let (id, media, rect, vector, ungrouped) = row?;
        if region_of(rect, named.get(&media)).is_some() {
            continue;
        }
        if let Some(vector) = usable(vector) {
            faces.push(UnknownFace {
                id,
                media,
                rect,
                vector: Some(vector),
                rejected: rejected.remove(&id).unwrap_or_default(),
                ungrouped,
            });
        }
    }
    Ok(faces)
}

/// Faces overlapping a named region, with the region's name: what the app knows people look like (REQ-032-08).
pub fn known_faces(conn: &Connection, detector: &str, recognizer: &str) -> Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.media_id, f.x, f.y, f.w, f.h, e.vector FROM detected_faces f
         JOIN face_embeddings e ON e.face_id = f.id AND e.recognizer = ?2
         WHERE f.detector = ?1 AND f.media_id IN (SELECT media_id FROM media_faces)",
    )?;
    let rows = stmt.query_map(params![detector, recognizer], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            [row.get::<_, f32>(1)?, row.get(2)?, row.get(3)?, row.get(4)?],
            row.get::<_, Vec<u8>>(5)?,
        ))
    })?;
    let named = regions(conn, None)?;
    let mut known = Vec::new();
    for row in rows {
        let (media, rect, vector) = row?;
        if let (Some(region), Some(vector)) = (region_of(rect, named.get(&media)), usable(vector)) {
            known.push((region.name.clone(), vector));
        }
    }
    Ok(known)
}

pub fn set_state(conn: &Connection, face: i64, state: FaceState) -> Result<()> {
    conn.prepare_cached("UPDATE detected_faces SET state = ?2 WHERE id = ?1")?
        .execute(params![face, state.as_str()])?;
    Ok(())
}

/// Remembers that `face` isn't `name` (REQ-032-10).
pub fn reject(conn: &Connection, face: i64, name: &str) -> Result<()> {
    conn.prepare_cached("INSERT OR IGNORE INTO face_rejections (face_id, name) VALUES (?1, ?2)")?
        .execute(params![face, name])?;
    Ok(())
}

/// The detected face lying on a region of `media`, for what removing a name should remember (REQ-032-13).
pub fn face_on_region(conn: &Connection, media: i64, detector: &str, rect: [f32; 4]) -> Result<Option<i64>> {
    let mut stmt =
        conn.prepare_cached("SELECT id, x, y, w, h FROM detected_faces WHERE media_id = ?1 AND detector = ?2")?;
    let rows = stmt.query_map(params![media, detector], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            [row.get::<_, f32>(1)?, row.get(2)?, row.get(3)?, row.get(4)?],
        ))
    })?;
    for row in rows {
        let (id, found) = row?;
        if same_face(found, rect) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Where a person's regions are on a photo.
pub fn regions_named(conn: &Connection, media: i64, name: &str) -> Result<Vec<[f32; 4]>> {
    Ok(conn
        .prepare_cached("SELECT x, y, w, h FROM media_faces WHERE media_id = ?1 AND name = ?2")?
        .query_map(params![media, name], |row| {
            Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?])
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Whether a photo was looked at by the chosen detection model (REQ-032-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceScan {
    /// No detection model is chosen, or it isn't a photo.
    #[default]
    NotLooking,
    NotYet,
    /// Looked at, with this many faces found, not counting what the user said isn't one.
    Done(u32),
}

pub fn scan_state(conn: &Connection, media: i64) -> Result<FaceScan> {
    let Some(detector) = chosen(conn, DETECTOR_KEY)? else {
        return Ok(FaceScan::NotLooking);
    };
    let scanned: Option<(String, bool)> = conn
        .prepare_cached(
            "SELECT m.kind, s.media_id IS NOT NULL AND s.size = m.size AND s.mtime = m.mtime FROM media m
             LEFT JOIN faces_scanned s ON s.media_id = m.id AND s.detector = ?2 WHERE m.id = ?1",
        )?
        .query_row(params![media, detector], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    Ok(match scanned {
        Some((kind, true)) if kind == "image" => FaceScan::Done(
            conn.prepare_cached(
                "SELECT count(*) FROM detected_faces WHERE media_id = ?1 AND detector = ?2 AND state <> 'not_face'",
            )?
            .query_row(params![media, detector], |row| row.get(0))?,
        ),
        Some((kind, false)) if kind == "image" => FaceScan::NotYet,
        _ => FaceScan::NotLooking,
    })
}

/// Has one photo, or all, looked at again. What was found stays until then, and what the user decided carries over
/// (REQ-032-22).
pub fn look_again(conn: &Connection, media: Option<i64>) -> Result<()> {
    match media {
        Some(media) => conn.execute("DELETE FROM faces_scanned WHERE media_id = ?1", [media])?,
        None => conn.execute("DELETE FROM faces_scanned", [])?,
    };
    Ok(())
}

/// Forgets a face; looking at its photo again may find it anew (REQ-032-30).
pub fn delete_face(conn: &Connection, face: i64) -> Result<()> {
    conn.prepare_cached("DELETE FROM detected_faces WHERE id = ?1")?
        .execute([face])?;
    Ok(())
}

/// Removes everything stored about faces, never the named regions of the files (REQ-032-04).
pub fn delete_all(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM detected_faces", [])?;
    conn.execute("DELETE FROM faces_scanned", [])?;
    Ok(())
}

/// The file of a media item.
pub fn photo(conn: &Connection, media: i64) -> Result<Option<FacePhoto>> {
    let sql = format!("SELECT {PHOTO_COLUMNS} FROM media m JOIN library_roots r ON r.id = m.root_id WHERE m.id = ?1");
    Ok(conn
        .prepare_cached(&sql)?
        .query_row([media], photo_from_row)
        .optional()?)
}

/// A face without a name as the viewer shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownFace {
    pub id: i64,
    pub rect: [f32; 4],
    pub suggestion: Option<String>,
    /// A second person about as likely (REQ-032-36).
    pub alternative: Option<String>,
}

/// The faces without a name on a photo, each with the person it looks most like (REQ-032-09, REQ-032-10).
pub fn shown_faces(
    conn: &Connection,
    media: i64,
    detector: &str,
    recognizer: Option<&str>,
    threshold: f32,
) -> Result<Vec<ShownFace>> {
    let recognizer = recognizer.unwrap_or_default();
    let faces = unknown_faces_on(conn, media, detector, recognizer)?;
    if faces.is_empty() {
        return Ok(Vec::new());
    }
    let known = known_faces(conn, detector, recognizer)?;
    let known: Vec<(&str, &[f32])> = known
        .iter()
        .map(|(name, vector)| (name.as_str(), vector.as_slice()))
        .collect();
    Ok(faces
        .into_iter()
        .map(|face| {
            let rejected: Vec<&str> = face.rejected.iter().map(String::as_str).collect();
            let mut likely = face
                .vector
                .as_deref()
                .map(|vector| suggestions(vector, &known, threshold, &rejected))
                .unwrap_or_default()
                .into_iter()
                .map(|(name, _)| name.to_owned());
            ShownFace {
                id: face.id,
                rect: face.rect,
                suggestion: likely.next(),
                alternative: likely.next(),
            }
        })
        .collect())
}

/// One of a group's faces.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupFace {
    pub id: i64,
    pub media: i64,
    pub rect: [f32; 4],
    /// A second person about as likely as the group's (REQ-032-36).
    pub alternative: Option<String>,
}

/// The faces of _Unconfirmed faces_: those with a suggestion by person (REQ-032-24), the others in groups of similar
/// faces (REQ-032-15), without those taken out of them (REQ-032-25), largest first; then those in no group
/// (REQ-032-28).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceGroups {
    pub suggested: Vec<(String, Vec<GroupFace>)>,
    pub unknown: Vec<Vec<GroupFace>>,
    pub others: Vec<GroupFace>,
}

pub fn face_groups(conn: &Connection, detector: &str, recognizer: &str, threshold: f32) -> Result<FaceGroups> {
    const MIN_GROUP: usize = 3;
    let faces = unknown_faces(conn, detector, recognizer)?;
    let known = known_faces(conn, detector, recognizer)?;
    let known: Vec<(&str, &[f32])> = known
        .iter()
        .map(|(name, vector)| (name.as_str(), vector.as_slice()))
        .collect();
    let shown = |face: &UnknownFace| GroupFace {
        id: face.id,
        media: face.media,
        rect: face.rect,
        alternative: None,
    };
    let mut suggested: HashMap<&str, Vec<GroupFace>> = HashMap::new();
    let mut rest = Vec::new();
    let mut others = Vec::new();
    for face in &faces {
        let rejected: Vec<&str> = face.rejected.iter().map(String::as_str).collect();
        let likely = face
            .vector
            .as_deref()
            .map(|vector| suggestions(vector, &known, threshold, &rejected))
            .unwrap_or_default();
        match likely.first() {
            Some((name, _)) => suggested.entry(name).or_default().push(GroupFace {
                alternative: likely.get(1).map(|(other, _)| (*other).to_owned()),
                ..shown(face)
            }),
            None if !face.ungrouped => rest.push(face),
            None => others.push(face),
        }
    }
    let mut suggested: Vec<(String, Vec<GroupFace>)> = suggested
        .into_iter()
        .map(|(name, faces)| (name.to_owned(), faces))
        .collect();
    suggested.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    let vectors: Vec<&[f32]> = rest.iter().filter_map(|face| face.vector.as_deref()).collect();
    let groups = group(&vectors, threshold, MIN_GROUP);
    let mut grouped = vec![false; rest.len()];
    for &index in groups.iter().flatten() {
        grouped[index] = true;
    }
    others.extend(
        rest.iter()
            .zip(&grouped)
            .filter(|(_, grouped)| !**grouped)
            .map(|(face, _)| *face),
    );
    others.sort_by_key(|face| face.id);
    let unknown = groups
        .into_iter()
        .map(|members| members.into_iter().map(|index| shown(rest[index])).collect())
        .collect();
    let others = others.into_iter().map(shown).collect();
    Ok(FaceGroups {
        suggested,
        unknown,
        others,
    })
}

/// Faces the user ignored or said aren't faces (REQ-032-30), oldest first. They need no recognition model: a face
/// decided before it was recognized has no vector. A face on a named region is that person instead (REQ-032-08).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecidedFaces {
    pub ignored: Vec<GroupFace>,
    pub not_faces: Vec<GroupFace>,
}

pub fn decided_faces(conn: &Connection, detector: &str) -> Result<DecidedFaces> {
    let named = regions(conn, None)?;
    let mut stmt = conn.prepare_cached(
        "SELECT id, media_id, x, y, w, h, state FROM detected_faces
         WHERE detector = ?1 AND state <> 'open' ORDER BY id",
    )?;
    let rows = stmt.query_map([detector], |row| {
        Ok((
            GroupFace {
                id: row.get(0)?,
                media: row.get(1)?,
                rect: [row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?],
                alternative: None,
            },
            FaceState::parse(&row.get::<_, String>(6)?),
        ))
    })?;
    let mut decided = DecidedFaces::default();
    for row in rows {
        let (face, state) = row?;
        if region_of(face.rect, named.get(&face.media)).is_some() {
            continue;
        }
        match state {
            FaceState::Ignored => decided.ignored.push(face),
            FaceState::NotAFace => decided.not_faces.push(face),
            FaceState::Open => {}
        }
    }
    Ok(decided)
}

/// Takes a face out of the groups of unknown faces for good; it stays unknown (REQ-032-25).
pub fn ungroup(conn: &Connection, face: i64) -> Result<()> {
    conn.prepare_cached("UPDATE detected_faces SET ungrouped = 1 WHERE id = ?1")?
        .execute([face])?;
    Ok(())
}

/// Whether known faces suggest the recognition model is set up wrongly (REQ-032-17).
pub fn model_seems_wrong(conn: &Connection, detector: &str, recognizer: &str) -> Result<bool> {
    let known = known_faces(conn, detector, recognizer)?;
    let known: Vec<(&str, &[f32])> = known
        .iter()
        .map(|(name, vector)| (name.as_str(), vector.as_slice()))
        .collect();
    Ok(tells_nobody_apart(&known))
}

/// A stored setting holding a model's hash, or the search's state.
pub fn chosen(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(crate::settings::value(conn, key)?.and_then(|value| value.as_str().map(str::to_owned)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{self, LibraryStatus};

    const ROOT: &str = if cfg!(windows) { "D:\\Photos" } else { "/media/Photos" };

    fn setup() -> (Connection, i64) {
        let conn = crate::open_in_memory().unwrap();
        let root = libraries::insert(&conn, ROOT, "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        (conn, root)
    }

    fn insert(conn: &Connection, root: i64, rel_path: &str, kind: &str) -> i64 {
        conn.query_row(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at)
             VALUES (?1, ?2, ?2, ?3, 7, 1000, 1) RETURNING id",
            params![root, rel_path, kind],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn face(x: f32) -> Detection {
        Detection {
            rect: [x, 0.1, 0.1, 0.1],
            landmarks: [[x, 0.5]; 5],
            score: 0.9,
        }
    }

    fn photo(conn: &Connection, detector: &str) -> FacePhoto {
        pending_photos(conn, detector, 10).unwrap().remove(0)
    }

    fn name_region(conn: &Connection, media: i64, name: &str, x: f32) {
        conn.execute(
            "INSERT INTO media_faces (media_id, name, x, y, w, h) VALUES (?1, ?2, ?3, 0.1, 0.1, 0.1)",
            params![media, name, x],
        )
        .unwrap();
    }

    /// REQ-032-23: only that photo's open faces this recognition model hasn't seen.
    #[test]
    fn req_032_23_only_the_open_faces_of_that_photo() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1), face(0.5), face(0.8)]).unwrap();
        let other = insert(&conn, root, "b.jpg", "image");
        let found = pending_photos(&conn, "d", 10).unwrap().remove(0);
        assert_eq!(found.media, other);
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();

        let faces = pending_faces_in(&conn, id, "d", "r").unwrap();
        assert_eq!(faces.len(), 3);
        assert!(faces.iter().all(|face| face.photo.media == id));
        set_state(&conn, faces[0].face, FaceState::Ignored).unwrap();
        store_embedding(&conn, faces[1].face, "r", &[1.0]).unwrap();
        let left = pending_faces_in(&conn, id, "d", "r").unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].face, faces[2].face);
        assert_eq!(pending_faces_in(&conn, id, "d", "other").unwrap().len(), 2);
        assert!(pending_faces_in(&conn, id, "another detector", "r").unwrap().is_empty());
    }

    #[test]
    fn req_032_05_works_through_photos_once_and_again_when_they_change() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        insert(&conn, root, "b.mp4", "video");
        assert_eq!(pending_photos(&conn, "d", 10).unwrap().len(), 1, "videos are skipped");

        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
        assert!(pending_photos(&conn, "d", 10).unwrap().is_empty());
        assert_eq!(
            progress(&conn, "d", None).unwrap(),
            FaceProgress {
                photos_done: 1,
                photos: 1,
                faces: 1,
                faces_recognized: 0,
            }
        );

        conn.execute("UPDATE media SET mtime = 2000 WHERE id = ?1", [id])
            .unwrap();
        assert_eq!(pending_photos(&conn, "d", 10).unwrap().len(), 1);
        assert_eq!(progress(&conn, "d", None).unwrap().photos_done, 0);
    }

    #[test]
    fn req_032_05_skips_offline_libraries() {
        let (conn, root) = setup();
        insert(&conn, root, "a.jpg", "image");
        conn.execute("UPDATE library_roots SET status = 'offline'", []).unwrap();
        assert!(pending_photos(&conn, "d", 10).unwrap().is_empty());
    }

    #[test]
    fn req_032_07_keeps_each_models_vectors_apart() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();

        let waiting = pending_faces(&conn, "d", "r1", 10).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].landmarks, [[0.1, 0.5]; 5]);
        store_embedding(&conn, waiting[0].face, "r1", &[1.0, 0.0]).unwrap();
        assert!(pending_faces(&conn, "d", "r1", 10).unwrap().is_empty());
        assert_eq!(
            pending_faces(&conn, "d", "r2", 10).unwrap().len(),
            1,
            "the other model hasn't seen it"
        );

        let unknown = unknown_faces_on(&conn, id, "d", "r1").unwrap();
        assert_eq!(unknown[0].vector, Some(vec![1.0, 0.0]));
        assert_eq!(unknown_faces_on(&conn, id, "d", "r2").unwrap()[0].vector, None);
        assert!(unknown_faces(&conn, "d", "r2").unwrap().is_empty());
    }

    #[test]
    fn req_032_08_a_face_on_a_named_region_is_that_person_until_the_file_changes() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1), face(0.5)]).unwrap();
        for waiting in pending_faces(&conn, "d", "r", 10).unwrap() {
            store_embedding(&conn, waiting.face, "r", &[waiting.landmarks[0][0]]).unwrap();
        }
        name_region(&conn, id, "Anna", 0.1);

        assert_eq!(known_faces(&conn, "d", "r").unwrap(), [("Anna".to_owned(), vec![0.1])]);
        let unknown = unknown_faces_on(&conn, id, "d", "r").unwrap();
        assert_eq!(unknown.len(), 1, "no second box");
        assert_eq!(unknown[0].rect[0], 0.5);
        assert_eq!(unknown_faces(&conn, "d", "r").unwrap().len(), 1);

        // Another program removed the region: the face is unknown again.
        conn.execute("DELETE FROM media_faces", []).unwrap();
        assert!(known_faces(&conn, "d", "r").unwrap().is_empty());
        assert_eq!(unknown_faces_on(&conn, id, "d", "r").unwrap().len(), 2);
    }

    #[test]
    fn req_032_10_remembers_who_a_face_is_not() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
        let face_id = unknown_faces_on(&conn, id, "d", "r").unwrap()[0].id;
        reject(&conn, face_id, "Anna").unwrap();
        reject(&conn, face_id, "Anna").unwrap();
        assert_eq!(unknown_faces_on(&conn, id, "d", "r").unwrap()[0].rejected, ["Anna"]);
    }

    /// Two photos of Anna, one of Bob, and an unknown face on each of three more.
    fn people(conn: &mut Connection, root: i64) -> Vec<i64> {
        let mut ids = Vec::new();
        for (n, vector) in [[1.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.9, 0.1], [0.9, 0.1], [0.9, 0.1]]
            .iter()
            .enumerate()
        {
            let id = insert(conn, root, &format!("{n}.jpg"), "image");
            let found = photo(conn, "d");
            store_scan(conn, &found, "d", &[face(0.1)]).unwrap();
            let waiting = pending_faces(conn, "d", "r", 1).unwrap().remove(0);
            store_embedding(conn, waiting.face, "r", vector).unwrap();
            ids.push(id);
        }
        name_region(conn, ids[0], "Anna", 0.1);
        name_region(conn, ids[1], "Anna", 0.1);
        name_region(conn, ids[2], "Bob", 0.1);
        ids
    }

    #[test]
    fn req_032_10_suggests_a_known_person_for_an_unknown_face() {
        let (mut conn, root) = setup();
        let ids = people(&mut conn, root);
        let shown = shown_faces(&conn, ids[3], "d", Some("r"), 0.5).unwrap();
        assert_eq!(shown[0].suggestion.as_deref(), Some("Anna"));

        reject(&conn, shown[0].id, "Anna").unwrap();
        let shown = shown_faces(&conn, ids[3], "d", Some("r"), 0.5).unwrap();
        assert_eq!(shown[0].suggestion, None, "Bob is too unlike");
        assert_eq!(
            shown_faces(&conn, ids[4], "d", None, 0.5).unwrap()[0].suggestion,
            None,
            "no recognition chosen"
        );
    }

    /// REQ-032-36 (#253): a face about as like Bob as Anna shows Bob with Anna as the alternative, in the viewer and
    /// in Bob's group; a face clearly like one has none.
    #[test]
    fn req_032_36_a_face_like_two_people_names_both() {
        let (mut conn, root) = setup();
        let ids = people(&mut conn, root);
        let both = insert(&conn, root, "both.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
        let waiting = pending_faces(&conn, "d", "r", 1).unwrap().remove(0);
        store_embedding(&conn, waiting.face, "r", &[0.7, 0.71]).unwrap();

        let shown = shown_faces(&conn, both, "d", Some("r"), 0.5).unwrap();
        assert_eq!(shown[0].suggestion.as_deref(), Some("Bob"));
        assert_eq!(shown[0].alternative.as_deref(), Some("Anna"));
        let clearly = shown_faces(&conn, ids[3], "d", Some("r"), 0.5).unwrap();
        assert_eq!(
            (clearly[0].suggestion.as_deref(), clearly[0].alternative.as_deref()),
            (Some("Anna"), None)
        );

        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        let (_, bob) = groups
            .suggested
            .iter()
            .find(|(name, _)| name == "Bob")
            .expect("Bob's group");
        assert_eq!(bob[0].alternative.as_deref(), Some("Anna"));
    }

    #[test]
    fn req_032_15_groups_only_faces_without_a_name() {
        let (mut conn, root) = setup();
        people(&mut conn, root);
        let ids = strangers(&mut conn, root, 3);
        let groups = face_groups(&conn, "d", "r", 0.5).unwrap().unknown;
        assert_eq!(groups.len(), 1, "named faces and those like Anna aren't in it");
        assert_eq!(groups[0].iter().map(|face| face.media).collect::<Vec<_>>(), ids);

        // With one ignored, too few remain.
        set_state(&conn, groups[0][0].id, FaceState::Ignored).unwrap();
        assert!(face_groups(&conn, "d", "r", 0.5).unwrap().unknown.is_empty());
    }

    /// Photos with one face each like nobody known (REQ-032-15).
    fn strangers(conn: &mut Connection, root: i64, count: usize) -> Vec<i64> {
        (0..count)
            .map(|n| {
                let id = insert(conn, root, &format!("stranger {n}.jpg"), "image");
                let found = photo(conn, "d");
                store_scan(conn, &found, "d", &[face(0.1)]).unwrap();
                let waiting = pending_faces(conn, "d", "r", 1).unwrap().remove(0);
                store_embedding(conn, waiting.face, "r", &[0.0, -1.0]).unwrap();
                id
            })
            .collect()
    }

    /// REQ-032-24: faces with a suggestion form one group per person; a face that isn't the person leaves it.
    #[test]
    fn req_032_24_groups_suggested_faces_by_person() {
        let (mut conn, root) = setup();
        let ids = people(&mut conn, root);
        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        assert!(groups.unknown.is_empty(), "{groups:?}");
        assert_eq!(groups.suggested.len(), 1);
        let (name, faces) = &groups.suggested[0];
        assert_eq!(name, "Anna");
        assert_eq!(faces.iter().map(|face| face.media).collect::<Vec<_>>(), ids[3..6]);

        reject(&conn, faces[0].id, "Anna").unwrap();
        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        assert_eq!(groups.suggested[0].1.len(), 2);
        assert!(groups.unknown.is_empty(), "one face alone is no group");
    }

    /// REQ-032-25: a face taken out of its group isn't grouped again, and stays an unknown face with its suggestion.
    #[test]
    fn req_032_25_a_face_taken_out_stays_unknown() {
        let (mut conn, root) = setup();
        let anna = people(&mut conn, root);
        let ids = strangers(&mut conn, root, 4);
        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        assert_eq!(groups.unknown[0].len(), 4);
        let out = groups.unknown[0][0].clone();
        ungroup(&conn, out.id).unwrap();

        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        assert_eq!(groups.unknown.len(), 1);
        assert!(groups.unknown[0].iter().all(|face| face.id != out.id), "{groups:?}");
        assert_eq!(groups.unknown[0].len(), 3);
        assert!(ids.contains(&out.media));
        let shown = shown_faces(&conn, out.media, "d", Some("r"), 0.5).unwrap();
        assert_eq!(
            shown.iter().map(|face| face.id).collect::<Vec<_>>(),
            [out.id],
            "still in the viewer"
        );

        // Taken out, a face like Anna is still one to confirm.
        let like_anna = face_groups(&conn, "d", "r", 0.5).unwrap().suggested[0].1[0].clone();
        ungroup(&conn, like_anna.id).unwrap();
        let suggested = face_groups(&conn, "d", "r", 0.5).unwrap().suggested;
        assert!(suggested[0].1.iter().any(|face| face.id == like_anna.id));
        assert_eq!(like_anna.media, anna[3]);
    }

    #[test]
    fn req_032_17_a_model_telling_people_apart_seems_right() {
        let (mut conn, root) = setup();
        people(&mut conn, root);
        assert!(!model_seems_wrong(&conn, "d", "r").unwrap(), "too few to say");
        for n in 6..8 {
            let id = insert(&conn, root, &format!("{n}.jpg"), "image");
            let found = photo(&conn, "d");
            store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
            let waiting = pending_faces(&conn, "d", "r", 1).unwrap().remove(0);
            store_embedding(&conn, waiting.face, "r", &[0.0, 1.0]).unwrap();
            name_region(&conn, id, "Bob", 0.1);
        }
        let more = insert(&conn, root, "8.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
        let waiting = pending_faces(&conn, "d", "r", 1).unwrap().remove(0);
        store_embedding(&conn, waiting.face, "r", &[1.0, 0.0]).unwrap();
        name_region(&conn, more, "Anna", 0.1);
        assert!(!model_seems_wrong(&conn, "d", "r").unwrap());

        conn.execute("UPDATE face_embeddings SET vector = ?1", [vector_to_blob(&[1.0, 0.0])])
            .unwrap();
        assert!(model_seems_wrong(&conn, "d", "r").unwrap());
    }

    #[test]
    fn req_032_11_ignored_faces_and_non_faces_stay_gone_when_looked_at_again() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1), face(0.5)]).unwrap();
        let faces = unknown_faces_on(&conn, id, "d", "r").unwrap();
        set_state(&conn, faces[0].id, FaceState::Ignored).unwrap();
        set_state(&conn, faces[1].id, FaceState::NotAFace).unwrap();
        assert!(unknown_faces_on(&conn, id, "d", "r").unwrap().is_empty());
        assert!(pending_faces(&conn, "d", "r", 10).unwrap().is_empty());

        store_scan(&mut conn, &found, "d", &[face(0.1), face(0.5)]).unwrap();
        assert!(unknown_faces_on(&conn, id, "d", "r").unwrap().is_empty());
    }

    /// REQ-032-28: unknown faces in no group, taken out or with too few alike, are the page's other faces; a face
    /// with a suggestion isn't among them.
    #[test]
    fn req_032_28_faces_in_no_group_are_the_other_unknown_faces() {
        let (mut conn, root) = setup();
        people(&mut conn, root);
        let ids = strangers(&mut conn, root, 4);
        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        assert!(groups.others.is_empty(), "{groups:?}");

        let out = groups.unknown[0][0].clone();
        ungroup(&conn, out.id).unwrap();
        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        assert_eq!(groups.others, [out]);

        // Two more out: a group of one is no group, so all four are others.
        for face in &groups.unknown[0][..2] {
            ungroup(&conn, face.id).unwrap();
        }
        let groups = face_groups(&conn, "d", "r", 0.5).unwrap();
        assert!(groups.unknown.is_empty());
        let mut others: Vec<i64> = groups.others.iter().map(|face| face.media).collect();
        others.sort();
        assert_eq!(others, ids);
        assert_eq!(groups.suggested.len(), 1, "a face like Anna is still to confirm");
    }

    /// REQ-032-30: decided faces are listed by decision, also before recognition; they can be undone, swapped and
    /// deleted, and a deleted one is found again only by looking again.
    #[test]
    fn req_032_30_lists_decided_faces_to_undo_swap_or_delete() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1), face(0.5), face(0.8)]).unwrap();
        let faces = unknown_faces_on(&conn, id, "d", "r").unwrap();
        set_state(&conn, faces[0].id, FaceState::Ignored).unwrap();
        set_state(&conn, faces[1].id, FaceState::NotAFace).unwrap();
        set_state(&conn, faces[2].id, FaceState::NotAFace).unwrap();

        let decided = decided_faces(&conn, "d").unwrap();
        let ids = |faces: &[GroupFace]| faces.iter().map(|face| face.id).collect::<Vec<_>>();
        assert_eq!(ids(&decided.ignored), [faces[0].id]);
        assert_eq!(ids(&decided.not_faces), [faces[1].id, faces[2].id]);
        assert_eq!(decided.ignored[0].media, id);
        assert_eq!(decided.ignored[0].rect, faces[0].rect);
        assert!(decided_faces(&conn, "another detector").unwrap().ignored.is_empty());

        set_state(&conn, faces[0].id, FaceState::Open).unwrap();
        set_state(&conn, faces[1].id, FaceState::Ignored).unwrap();
        delete_face(&conn, faces[2].id).unwrap();
        let decided = decided_faces(&conn, "d").unwrap();
        assert_eq!(ids(&decided.ignored), [faces[1].id]);
        assert!(decided.not_faces.is_empty());
        let open = unknown_faces_on(&conn, id, "d", "r").unwrap();
        assert_eq!(ids_of(&open), [faces[0].id], "back to unknown, the deleted one gone");
        assert_eq!(pending_faces(&conn, "d", "r", 10).unwrap().len(), 1);

        // A named region on a decided face makes it that person, not a decision.
        name_region(&conn, id, "Bob", 0.5);
        assert!(decided_faces(&conn, "d").unwrap().ignored.is_empty());

        look_again(&conn, Some(id)).unwrap();
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1), face(0.5), face(0.8)]).unwrap();
        let open = unknown_faces_on(&conn, id, "d", "r").unwrap();
        assert_eq!(open.len(), 2, "the deleted face is found again: {open:?}");
    }

    fn ids_of(faces: &[UnknownFace]) -> Vec<i64> {
        faces.iter().map(|face| face.id).collect()
    }

    #[test]
    fn req_032_16_decisions_carry_over_to_another_detector_and_earlier_work_stays() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d1");
        store_scan(&mut conn, &found, "d1", &[face(0.1), face(0.5)]).unwrap();
        let faces = unknown_faces_on(&conn, id, "d1", "r").unwrap();
        set_state(&conn, faces[0].id, FaceState::Ignored).unwrap();
        reject(&conn, faces[1].id, "Bob").unwrap();

        assert_eq!(pending_photos(&conn, "d2", 10).unwrap().len(), 1);
        store_scan(&mut conn, &found, "d2", &[face(0.105), face(0.505)]).unwrap();
        let moved = unknown_faces_on(&conn, id, "d2", "r").unwrap();
        assert_eq!(moved.len(), 1, "the ignored one stays ignored");
        assert_eq!(moved[0].rejected, ["Bob"]);

        // Going back needs no new look.
        assert!(pending_photos(&conn, "d1", 10).unwrap().is_empty());
        assert_eq!(unknown_faces_on(&conn, id, "d1", "r").unwrap().len(), 1);
    }

    #[test]
    fn req_032_04_deletes_all_face_data_but_no_named_regions() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
        let waiting = pending_faces(&conn, "d", "r", 10).unwrap();
        store_embedding(&conn, waiting[0].face, "r", &[1.0]).unwrap();
        reject(&conn, waiting[0].face, "Anna").unwrap();
        name_region(&conn, id, "Bob", 0.7);

        delete_all(&conn).unwrap();

        for table in ["detected_faces", "face_embeddings", "face_rejections", "faces_scanned"] {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
        let named: i64 = conn
            .query_row("SELECT count(*) FROM media_faces", [], |row| row.get(0))
            .unwrap();
        assert_eq!(named, 1);
    }

    #[test]
    fn req_032_21_says_whether_a_photo_was_looked_at() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let video = insert(&conn, root, "b.mp4", "video");
        assert_eq!(scan_state(&conn, id).unwrap(), FaceScan::NotLooking, "no model chosen");

        crate::settings::set_value(&conn, DETECTOR_KEY, &serde_json::json!("d")).unwrap();
        assert_eq!(scan_state(&conn, id).unwrap(), FaceScan::NotYet);
        assert_eq!(scan_state(&conn, video).unwrap(), FaceScan::NotLooking);

        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1), face(0.5)]).unwrap();
        assert_eq!(scan_state(&conn, id).unwrap(), FaceScan::Done(2));

        conn.execute("UPDATE media SET mtime = 2000 WHERE id = ?1", [id])
            .unwrap();
        assert_eq!(scan_state(&conn, id).unwrap(), FaceScan::NotYet, "it changed");
    }

    #[test]
    fn req_032_22_looks_again_and_decisions_stay() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let other = insert(&conn, root, "b.jpg", "image");
        for found in pending_photos(&conn, "d", 10).unwrap() {
            store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
        }
        let ignored = unknown_faces_on(&conn, id, "d", "r").unwrap()[0].id;
        set_state(&conn, ignored, FaceState::Ignored).unwrap();

        look_again(&conn, Some(id)).unwrap();
        let waiting = pending_photos(&conn, "d", 10).unwrap();
        assert_eq!(waiting.iter().map(|photo| photo.media).collect::<Vec<_>>(), [id]);
        store_scan(&mut conn, &waiting[0], "d", &[face(0.1)]).unwrap();
        assert!(
            unknown_faces_on(&conn, id, "d", "r").unwrap().is_empty(),
            "still ignored"
        );

        look_again(&conn, None).unwrap();
        assert_eq!(pending_photos(&conn, "d", 10).unwrap().len(), 2);
        assert_eq!(
            unknown_faces_on(&conn, other, "d", "r").unwrap().len(),
            1,
            "found stays until then"
        );
    }

    #[test]
    fn faces_go_with_their_media_item() {
        let (mut conn, root) = setup();
        let id = insert(&conn, root, "a.jpg", "image");
        let found = photo(&conn, "d");
        store_scan(&mut conn, &found, "d", &[face(0.1)]).unwrap();
        conn.execute("DELETE FROM media WHERE id = ?1", [id]).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM detected_faces", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
