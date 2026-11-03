//! Writes face regions the user confirmed, drew, renamed or removed into a file or its sidecar (REQ-031-12 …
//! REQ-031-14), as exiftool's structs read back and changed, so regions and fields the app doesn't know survive.

use crate::rating::{WriteError, existing_sidecar, library_path, path_arg};
use domain::face_models::same_face;
use domain::faces::stored_rect;
use serde_json::{Value, json};
use std::path::Path;

const MWG: &str = "XMP-mwg-rs:RegionInfo";
const MICROSOFT: &str = "XMP-MP:RegionInfoMP";
const PEOPLE: &str = "XMP-iptcExt:PersonInImage";

/// What changes, with rectangles as fractions of the upright photo.
#[derive(Debug, Clone, PartialEq)]
pub enum RegionChange {
    Add {
        name: String,
        rect: [f32; 4],
    },
    /// Without a rectangle: all of the person's regions on the photo.
    Remove {
        name: String,
        rect: Option<[f32; 4]>,
    },
    Rename {
        from: String,
        to: String,
        rect: Option<[f32; 4]>,
    },
}

/// A number exiftool may give as text.
fn number(value: &Value) -> Option<f32> {
    value
        .as_f64()
        .map(|n| n as f32)
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

/// A region of the MWG struct as `x, y, w, h`.
fn mwg_rect(region: &Value) -> Option<[f32; 4]> {
    let area = region.get("Area")?;
    let (cx, cy) = (number(area.get("X")?)?, number(area.get("Y")?)?);
    let (w, h) = (number(area.get("W")?)?, number(area.get("H")?)?);
    Some([cx - w / 2.0, cy - h / 2.0, w, h])
}

/// A region of the Microsoft struct as `x, y, w, h`.
fn microsoft_rect(region: &Value) -> Option<[f32; 4]> {
    let rectangle = region.get("Rectangle")?.as_str()?;
    let mut numbers = rectangle.split(',').map(|part| part.trim().parse().ok());
    Some([numbers.next()??, numbers.next()??, numbers.next()??, numbers.next()??])
}

/// Whether `region` is the one meant: a person's by name, and at `rect` if given.
fn is_meant(name_in_file: &str, name: &str, at: Option<[f32; 4]>, rect: Option<[f32; 4]>) -> bool {
    name_in_file == name
        && match rect {
            Some(rect) => at.is_some_and(|at| same_face(at, rect)),
            None => true,
        }
}

fn is_face(region: &Value) -> bool {
    region
        .get("Type")
        .is_none_or(|kind| text(kind).eq_ignore_ascii_case("face"))
}

fn rounded(n: f32) -> f64 {
    (f64::from(n) * 100_000.0).round() / 100_000.0
}

/// `info`, the MWG struct, with the change; rectangles on the stored photo. `true` if something changed.
fn change_mwg(info: &mut Value, change: &RegionChange, size: Option<(u32, u32)>) -> bool {
    if !info.is_object() {
        *info = json!({});
    }
    if info.get("RegionList").is_none_or(|list| !list.is_array()) {
        info["RegionList"] = json!([]);
    }
    if info.get("AppliedToDimensions").is_none()
        && let Some((w, h)) = size
    {
        info["AppliedToDimensions"] = json!({ "W": w, "H": h, "Unit": "pixel" });
    }
    let Some(list) = info["RegionList"].as_array_mut() else {
        return false;
    };
    match change {
        RegionChange::Add { name, rect } => {
            let [x, y, w, h] = *rect;
            list.push(json!({
                "Area": {
                    "X": rounded(x + w / 2.0),
                    "Y": rounded(y + h / 2.0),
                    "W": rounded(w),
                    "H": rounded(h),
                    "Unit": "normalized",
                },
                "Name": name,
                "Type": "Face",
            }));
            true
        }
        RegionChange::Remove { name, rect } => {
            let before = list.len();
            list.retain(|region| {
                !(is_face(region)
                    && is_meant(
                        &text(region.get("Name").unwrap_or(&Value::Null)),
                        name,
                        mwg_rect(region),
                        *rect,
                    ))
            });
            list.len() != before
        }
        RegionChange::Rename { from, to, rect } => {
            let mut changed = false;
            for region in list.iter_mut() {
                let meant = is_face(region)
                    && is_meant(
                        &text(region.get("Name").unwrap_or(&Value::Null)),
                        from,
                        mwg_rect(region),
                        *rect,
                    );
                if meant {
                    region["Name"] = json!(to);
                    changed = true;
                }
            }
            changed
        }
    }
}

/// `info`, the Microsoft struct, with the change. It is only written where it exists (REQ-031-14).
fn change_microsoft(info: &mut Value, change: &RegionChange) -> bool {
    let Some(list) = info.get_mut("Regions").and_then(Value::as_array_mut) else {
        return false;
    };
    let name_of = |region: &Value| text(region.get("PersonDisplayName").unwrap_or(&Value::Null));
    match change {
        RegionChange::Add { name, rect } => {
            let [x, y, w, h] = rect.map(rounded);
            list.push(json!({ "PersonDisplayName": name, "Rectangle": format!("{x}, {y}, {w}, {h}") }));
            true
        }
        RegionChange::Remove { name, rect } => {
            let before = list.len();
            list.retain(|region| !is_meant(&name_of(region), name, microsoft_rect(region), *rect));
            list.len() != before
        }
        RegionChange::Rename { from, to, rect } => {
            let mut changed = false;
            for region in list.iter_mut() {
                if is_meant(&name_of(region), from, microsoft_rect(region), *rect) {
                    region["PersonDisplayName"] = json!(to);
                    changed = true;
                }
            }
            changed
        }
    }
}

/// Face regions of `name` still in the MWG struct.
fn still_named(info: &Value, name: &str) -> bool {
    info.get("RegionList").and_then(Value::as_array).is_some_and(|list| {
        list.iter()
            .any(|region| is_face(region) && text(region.get("Name").unwrap_or(&Value::Null)) == name)
    })
}

fn read_tags(exiftool: &Path, file: &Path) -> Result<Value, WriteError> {
    let args: Vec<String> = [
        "-charset".to_owned(),
        "filename=utf8".to_owned(),
        "-json".to_owned(),
        "-struct".to_owned(),
        "-Orientation#".to_owned(),
        format!("-{MWG}"),
        format!("-{MICROSOFT}"),
        path_arg(file),
    ]
    .to_vec();
    let output = crate::exiftool::run_once(exiftool, None, &args).map_err(WriteError::Exiftool)?;
    let parsed: Value = serde_json::from_str(&output).unwrap_or_default();
    Ok(parsed.get(0).cloned().unwrap_or_default())
}

fn has_regions(tags: &Value) -> bool {
    tags.get("RegionInfo")
        .and_then(|info| info.get("RegionList"))
        .and_then(Value::as_array)
        .is_some_and(|list| !list.is_empty())
}

/// Turns a change's rectangles from the upright photo to the stored one.
fn on_stored(change: &RegionChange, orientation: u8) -> RegionChange {
    let turned = |rect: &Option<[f32; 4]>| rect.map(|rect| stored_rect(rect, orientation));
    match change {
        RegionChange::Add { name, rect } => RegionChange::Add {
            name: name.clone(),
            rect: stored_rect(*rect, orientation),
        },
        RegionChange::Remove { name, rect } => RegionChange::Remove {
            name: name.clone(),
            rect: turned(rect),
        },
        RegionChange::Rename { from, to, rect } => RegionChange::Rename {
            from: from.clone(),
            to: to.clone(),
            rect: turned(rect),
        },
    }
}

/// Applies `change` to the photo `rel_path` of the library in `library`. A new region goes into the file, or into the
/// sidecar when only that has regions, since a file's regions hide its sidecar's (REQ-004-13). Changing or removing
/// touches wherever the region is. The date modified stays, and a file that can't be written stays as it was.
pub fn write(
    exiftool: &Path,
    library: &Path,
    rel_path: &str,
    sidecar_orientation: Option<u8>,
    change: &RegionChange,
) -> Result<(), WriteError> {
    let file = library_path(library, rel_path).ok_or(WriteError::UnsafePath)?;
    crate::replace::recover(&file)?;
    if !file.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
    }
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let folder = file.parent().ok_or(WriteError::UnsafePath)?;
    let sidecar = existing_sidecar(folder, name)?;
    let file_tags = read_tags(exiftool, &file)?;
    let sidecar_tags = match &sidecar {
        Some(sidecar) => Some((sidecar.clone(), read_tags(exiftool, sidecar)?)),
        None => None,
    };
    let orientation = file_tags
        .get("Orientation")
        .and_then(number)
        .map(|n| n as u8)
        .or(sidecar_orientation)
        .unwrap_or(1);
    let stored = on_stored(change, orientation);
    let adding = matches!(change, RegionChange::Add { .. });
    let into_sidecar = adding && !has_regions(&file_tags);
    let mut targets = vec![(file.clone(), file_tags)];
    match sidecar_tags {
        Some((path, tags)) if !adding => targets.push((path, tags)),
        Some((path, tags)) if into_sidecar && has_regions(&tags) => targets = vec![(path, tags)],
        _ => {}
    }
    let size = image::image_dimensions(&file).ok();
    let mut writes = Vec::new();
    for (target, tags) in targets {
        let mut info = tags.get("RegionInfo").cloned().unwrap_or(Value::Null);
        let mut microsoft = tags.get("RegionInfoMP").cloned().unwrap_or(Value::Null);
        let keep_mwg = !adding && info.is_null();
        let mwg_changed = !keep_mwg && change_mwg(&mut info, &stored, size);
        let microsoft_changed = change_microsoft(&mut microsoft, &stored);
        if mwg_changed || microsoft_changed {
            writes.push((target, info, mwg_changed, microsoft, microsoft_changed));
        }
    }
    for (target, _, _, _, _) in &writes {
        if std::fs::metadata(target)?.permissions().readonly() {
            return Err(WriteError::ReadOnly);
        }
    }
    let structs = tempfile::tempdir()?;
    for (target, info, mwg_changed, microsoft, microsoft_changed) in writes {
        let mut args = vec!["-charset".to_owned(), "filename=utf8".to_owned()];
        let mut json = json!({ "SourceFile": "*" });
        let is_empty = |info: &Value, key: &str| info.get(key).and_then(Value::as_array).is_none_or(Vec::is_empty);
        if mwg_changed {
            if is_empty(&info, "RegionList") {
                args.push(format!("-{MWG}="));
            } else {
                json["RegionInfo"] = info.clone();
            }
        }
        if microsoft_changed {
            if is_empty(&microsoft, "Regions") {
                args.push(format!("-{MICROSOFT}="));
            } else {
                json["RegionInfoMP"] = microsoft;
            }
        }
        if json.as_object().is_some_and(|object| object.len() > 1) {
            // Through a file, so names need no escaping for exiftool's struct syntax.
            let path = structs.path().join("regions.json");
            std::fs::write(&path, json!([json]).to_string())?;
            args.push(format!("-json={}", path_arg(&path)));
        }
        let (added, removed): (Option<&str>, Option<&str>) = match change {
            RegionChange::Add { name, .. } => (Some(name), None),
            RegionChange::Remove { name, .. } => (None, Some(name)),
            RegionChange::Rename { from, to, .. } => (Some(to), Some(from)),
        };
        if let Some(name) = removed.filter(|name| !still_named(&info, name)) {
            args.push(format!("-{PEOPLE}-={name}"));
        }
        if let Some(name) = added {
            // Removing first keeps a name that is there already from appearing twice.
            args.push(format!("-{PEOPLE}-={name}"));
            args.push(format!("-{PEOPLE}+={name}"));
        }
        crate::replace::write_metadata(exiftool, None, &args, &target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    struct Library {
        dir: tempfile::TempDir,
        exiftool: PathBuf,
    }

    fn library() -> Option<Library> {
        drop(crate::exiftool::tests::exiftool()?);
        Some(Library {
            dir: tempfile::tempdir().unwrap(),
            exiftool: crate::exiftool::find_exiftool().unwrap(),
        })
    }

    const OTHERS: &str = "-XMP-mwg-rs:RegionInfo={AppliedToDimensions={W=64,H=32,Unit=pixel},RegionList=[\
         {Area={X=0.75,Y=0.25,W=0.2,H=0.2,Unit=normalized},Name=Anna,Type=Face},\
         {Area={X=0.25,Y=0.75,W=0.2,H=0.2,Unit=normalized},Name=Rex,Type=Pet},\
         {Area={X=0.25,Y=0.25,W=0.1,H=0.1,Unit=normalized},Name=Bob,Type=Face,Description=Keep me}]}";

    impl Library {
        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn photo(&self, name: &str, args: &[&str]) -> PathBuf {
            let path = self.path(name);
            image::RgbImage::from_pixel(64, 32, image::Rgb([200, 90, 40]))
                .save(&path)
                .unwrap();
            if !args.is_empty() {
                let status = Command::new(&self.exiftool)
                    .args(args)
                    .args(["-overwrite_original", "-q"])
                    .arg(&path)
                    .status()
                    .unwrap();
                assert!(status.success());
            }
            path
        }

        fn write(&self, name: &str, change: &RegionChange) {
            write(&self.exiftool, self.dir.path(), name, None, change).unwrap();
        }

        fn tags(&self, file: &Path) -> Value {
            let output = Command::new(&self.exiftool)
                .args([
                    "-json",
                    "-struct",
                    "-a",
                    "-XMP-mwg-rs:RegionInfo",
                    "-XMP-MP:RegionInfoMP",
                    "-PersonInImage",
                ])
                .arg(file)
                .output()
                .unwrap();
            serde_json::from_slice::<Value>(&output.stdout).unwrap()[0].clone()
        }
    }

    fn names(tags: &Value) -> Vec<String> {
        tags["RegionInfo"]["RegionList"].as_array().map_or(Vec::new(), |list| {
            list.iter().map(|region| text(&region["Name"])).collect()
        })
    }

    fn people(tags: &Value) -> Vec<String> {
        match &tags["PersonInImage"] {
            Value::Array(list) => list.iter().map(text).collect(),
            Value::Null => Vec::new(),
            one => vec![text(one)],
        }
    }

    fn add(name: &str) -> RegionChange {
        RegionChange::Add {
            name: name.into(),
            rect: [0.4, 0.4, 0.2, 0.2],
        }
    }

    #[test]
    fn req_031_12_a_new_region_goes_into_the_file_and_reads_back_where_it_was_drawn() {
        let Some(lib) = library() else { return };
        let file = lib.photo("a.jpg", &[]);
        lib.write(
            "a.jpg",
            &RegionChange::Add {
                name: "Anna Müller".into(),
                rect: [0.1, 0.2, 0.2, 0.4],
            },
        );
        let tags = lib.tags(&file);
        let region = &tags["RegionInfo"]["RegionList"][0];
        assert_eq!(text(&region["Name"]), "Anna Müller");
        assert_eq!(text(&region["Type"]), "Face");
        assert_eq!(text(&region["Area"]["Unit"]), "normalized");
        assert!(
            (number(&region["Area"]["X"]).unwrap() - 0.2).abs() < 1e-4,
            "the center: {region}"
        );
        assert!((number(&region["Area"]["Y"]).unwrap() - 0.4).abs() < 1e-4, "{region}");
        assert_eq!(number(&tags["RegionInfo"]["AppliedToDimensions"]["W"]), Some(64.0));
        assert_eq!(people(&tags), ["Anna Müller"]);
    }

    /// REQ-031-14: everything else in the list and the file stays, also what the app doesn't know.
    #[test]
    fn req_031_14_other_regions_and_fields_survive() {
        let Some(lib) = library() else { return };
        let file = lib.photo("a.jpg", &[OTHERS, "-XMP-dc:Title=A title", "-XMP-xmp:Rating=3"]);
        lib.write("a.jpg", &add("Clara"));
        let tags = lib.tags(&file);
        assert_eq!(names(&tags), ["Anna", "Rex", "Bob", "Clara"]);
        assert_eq!(text(&tags["RegionInfo"]["RegionList"][2]["Description"]), "Keep me");
        let rest = Command::new(&lib.exiftool)
            .args(["-s3", "-XMP-dc:Title", "-XMP-xmp:Rating"])
            .arg(&file)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&rest.stdout).lines().collect::<Vec<_>>(),
            ["A title", "3"]
        );
    }

    #[test]
    fn req_031_13_removes_and_renames_one_region_of_any_program() {
        let Some(lib) = library() else { return };
        let file = lib.photo(
            "a.jpg",
            &[
                OTHERS,
                "-XMP-iptcExt:PersonInImage=Anna",
                "-XMP-iptcExt:PersonInImage=Bob",
            ],
        );
        // A loose rectangle around Bob's region: another program's box differs from ours.
        lib.write(
            "a.jpg",
            &RegionChange::Rename {
                from: "Bob".into(),
                to: "Robert".into(),
                rect: Some([0.19, 0.19, 0.12, 0.12]),
            },
        );
        let tags = lib.tags(&file);
        assert_eq!(names(&tags), ["Anna", "Rex", "Robert"]);
        assert_eq!(people(&tags), ["Anna", "Robert"]);

        lib.write(
            "a.jpg",
            &RegionChange::Remove {
                name: "Anna".into(),
                rect: None,
            },
        );
        let tags = lib.tags(&file);
        assert_eq!(names(&tags), ["Rex", "Robert"], "a pet of the same list stays");
        assert_eq!(people(&tags), ["Robert"]);
    }

    #[test]
    fn req_031_13_a_person_with_another_region_stays_in_person_in_image() {
        let Some(lib) = library() else { return };
        let file = lib.photo("a.jpg", &[]);
        lib.write(
            "a.jpg",
            &RegionChange::Add {
                name: "Anna".into(),
                rect: [0.1, 0.1, 0.2, 0.2],
            },
        );
        lib.write(
            "a.jpg",
            &RegionChange::Add {
                name: "Anna".into(),
                rect: [0.6, 0.6, 0.2, 0.2],
            },
        );
        lib.write(
            "a.jpg",
            &RegionChange::Remove {
                name: "Anna".into(),
                rect: Some([0.1, 0.1, 0.2, 0.2]),
            },
        );
        let tags = lib.tags(&file);
        assert_eq!(names(&tags), ["Anna"]);
        assert_eq!(people(&tags), ["Anna"]);
        lib.write(
            "a.jpg",
            &RegionChange::Remove {
                name: "Anna".into(),
                rect: None,
            },
        );
        let tags = lib.tags(&file);
        assert!(names(&tags).is_empty(), "{tags}");
        assert!(people(&tags).is_empty(), "{tags}");
    }

    #[test]
    fn req_031_14_changes_the_microsoft_region_only_where_it_exists() {
        let Some(lib) = library() else { return };
        let plain = lib.photo("plain.jpg", &[]);
        lib.write("plain.jpg", &add("Anna"));
        assert_eq!(lib.tags(&plain)["RegionInfoMP"], Value::Null);

        let both = lib.photo(
            "both.jpg",
            &["-XMP-MP:RegionInfoMP={Regions=[{PersonDisplayName=Anna,Rectangle=0.65|, 0.15|, 0.2|, 0.2}]}"],
        );
        lib.write("both.jpg", &add("Clara"));
        let tags = lib.tags(&both);
        let list = tags["RegionInfoMP"]["Regions"].as_array().unwrap();
        assert_eq!(text(&list[1]["PersonDisplayName"]), "Clara");
        assert_eq!(text(&list[1]["Rectangle"]), "0.4, 0.4, 0.2, 0.2");

        lib.write(
            "both.jpg",
            &RegionChange::Remove {
                name: "Anna".into(),
                rect: None,
            },
        );
        let tags = lib.tags(&both);
        assert_eq!(tags["RegionInfoMP"]["Regions"].as_array().unwrap().len(), 1);
    }

    /// REQ-031-14: rectangles are for the upright photo, files store them for the turned one.
    #[test]
    fn req_031_14_stores_rectangles_for_the_files_orientation() {
        let Some(lib) = library() else { return };
        let file = lib.photo("a.jpg", &["-Orientation#=6"]);
        lib.write(
            "a.jpg",
            &RegionChange::Add {
                name: "Anna".into(),
                rect: [0.1, 0.2, 0.2, 0.4],
            },
        );
        let tags = lib.tags(&file);
        let area = &tags["RegionInfo"]["RegionList"][0]["Area"];
        // Read as REQ-004-13 does, it is where it was drawn.
        let [w, h] = [number(&area["W"]).unwrap(), number(&area["H"]).unwrap()];
        let [x, y] = [
            number(&area["X"]).unwrap() - w / 2.0,
            number(&area["Y"]).unwrap() - h / 2.0,
        ];
        let upright = domain::faces::upright_rect([x, y, w, h], 6);
        for (a, b) in upright.iter().zip([0.1, 0.2, 0.2, 0.4]) {
            assert!((a - b).abs() < 1e-3, "{upright:?}");
        }
    }

    #[test]
    fn req_031_14_uses_the_sidecar_when_only_it_has_regions() {
        let Some(lib) = library() else { return };
        let file = lib.photo("a.jpg", &[]);
        let sidecar = lib.path("a.jpg.xmp");
        let status = Command::new(&lib.exiftool)
            .args([OTHERS, "-q"])
            .arg(&sidecar)
            .status()
            .unwrap();
        assert!(status.success());
        lib.write("a.jpg", &add("Clara"));
        assert!(names(&lib.tags(&file)).is_empty());
        assert_eq!(names(&lib.tags(&sidecar)).last().map(String::as_str), Some("Clara"));

        lib.write(
            "a.jpg",
            &RegionChange::Remove {
                name: "Anna".into(),
                rect: None,
            },
        );
        assert!(!names(&lib.tags(&sidecar)).contains(&"Anna".to_owned()));
    }

    #[test]
    fn req_031_14_a_read_only_file_shows_why_and_stays() {
        let Some(lib) = library() else { return };
        let file = lib.photo("a.jpg", &[]);
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions).unwrap();
        let error = write(&lib.exiftool, lib.dir.path(), "a.jpg", None, &add("Anna"));
        assert!(matches!(error, Err(WriteError::ReadOnly)), "{error:?}");
        assert!(names(&lib.tags(&file)).is_empty());
    }

    /// REQ-031-14: what's written reads back through the indexer's own rules.
    #[test]
    fn req_031_14_is_read_back_by_the_indexing_rules() {
        let Some(lib) = library() else { return };
        let file = lib.photo("a.jpg", &[]);
        lib.write(
            "a.jpg",
            &RegionChange::Add {
                name: "Anna".into(),
                rect: [0.1, 0.2, 0.2, 0.4],
            },
        );
        let output = Command::new(&lib.exiftool)
            .args(["-j", "-struct", "-n"])
            .arg(&file)
            .output()
            .unwrap();
        let read: Value = serde_json::from_slice(&output.stdout).unwrap();
        let metadata = meta::rules::extract_metadata(read[0].as_object().unwrap(), None);
        assert_eq!(metadata.faces.len(), 1);
        assert_eq!(metadata.faces[0].name, "Anna");
        assert!((metadata.faces[0].x - 0.1).abs() < 1e-3 && (metadata.faces[0].h - 0.4).abs() < 1e-3);
        assert!(
            metadata
                .tags
                .iter()
                .any(|path| path.last().is_some_and(|tag| tag == "Anna")),
            "{:?}",
            metadata.tags
        );
    }
}
