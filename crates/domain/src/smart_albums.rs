//! Smart albums (spec 034): a search kept under a name. How a search is stored, and the name suggested for it.

use crate::media::MediaKind;
use crate::search::{GallerySearch, SortBy};
use chrono::NaiveDate;
use serde_json::{Map, Value, json};

/// A saved search.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartAlbum {
    pub id: i64,
    pub name: String,
    pub search: GallerySearch,
}

/// A name the user typed: trimmed, `None` if nothing is left (REQ-034-01, REQ-034-05).
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The name suggested for `search`: its tags (excluded ones with a minus) and file name, else _Smart album_
/// (REQ-034-01).
pub fn suggested_name(search: &GallerySearch) -> String {
    let leaf = |tag: &String| tag.rsplit('|').next().unwrap_or(tag).trim().to_string();
    let mut parts: Vec<String> = search.content.iter().map(|words| words.trim().to_string()).collect();
    parts.extend(search.tags.iter().map(leaf));
    parts.extend(search.exclude.iter().map(|tag| format!("-{}", leaf(tag))));
    parts.extend(search.name.iter().map(|name| name.trim().to_string()));
    parts.retain(|part| !part.is_empty() && part != "-");
    if parts.is_empty() {
        "Smart album".into()
    } else {
        parts.join(", ")
    }
}

/// How a search is stored (REQ-034-07): only what is set, so a default search is `{}`.
pub fn search_to_value(search: &GallerySearch) -> Value {
    let mut map = Map::new();
    let mut put = |key: &str, value: Value| {
        map.insert(key.into(), value);
    };
    if let Some(library) = search.library {
        put("library", json!(library));
    }
    if let Some(folder) = &search.folder {
        put("folder", json!(folder));
    }
    if let Some(kind) = search.kind {
        put("kind", json!(kind.as_str()));
    }
    if let Some(name) = &search.name {
        put("name", json!(name));
    }
    if !search.tags.is_empty() {
        put("tags", json!(search.tags));
    }
    if !search.exclude.is_empty() {
        put("exclude", json!(search.exclude));
    }
    if let Some(from) = search.from {
        put("from", json!(from.to_string()));
    }
    if let Some(to) = search.to {
        put("to", json!(to.to_string()));
    }
    if !search.ratings.is_empty() {
        put("ratings", json!(search.ratings));
    }
    if let Some(content) = &search.content {
        put("content", json!(content));
    }
    let default = GallerySearch::default();
    if search.sort_by != default.sort_by {
        put("sort_by", json!(search.sort_by.key()));
    }
    if search.descending != default.descending {
        put("descending", json!(search.descending));
    }
    Value::Object(map)
}

/// A stored search. What is missing or can't be read keeps its default, so an album outlives changes to the format.
pub fn search_from_value(value: &Value) -> GallerySearch {
    let mut search = GallerySearch::default();
    let Some(map) = value.as_object() else {
        return search;
    };
    let text = |key: &str| map.get(key).and_then(Value::as_str).map(str::to_string);
    let texts = |key: &str| -> Vec<String> {
        map.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    };
    let day = |key: &str| text(key).and_then(|day| day.parse::<NaiveDate>().ok());
    search.library = map.get("library").and_then(Value::as_i64);
    search.folder = text("folder");
    search.kind = text("kind").and_then(|kind| MediaKind::parse(&kind));
    search.name = text("name");
    search.content = text("content");
    search.tags = texts("tags");
    search.exclude = texts("exclude");
    search.from = day("from");
    search.to = day("to");
    search.ratings = map
        .get("ratings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .filter(|&rating| rating <= 5)
        .map(|rating| rating as u8)
        .collect();
    if let Some(sort_by) = text("sort_by").and_then(|key| SortBy::from_key(&key)) {
        search.sort_by = sort_by;
    }
    if let Some(descending) = map.get("descending").and_then(Value::as_bool) {
        search.descending = descending;
    }
    search
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> GallerySearch {
        GallerySearch {
            library: Some(3),
            folder: Some("2023/Rome".into()),
            kind: Some(MediaKind::Video),
            name: Some("IMG".into()),
            tags: vec!["Rome".into(), "People|Anna".into()],
            exclude: vec!["Blurry".into()],
            from: NaiveDate::from_ymd_opt(2023, 5, 1),
            to: NaiveDate::from_ymd_opt(2023, 5, 14),
            ratings: vec![0, 4, 5],
            media: Vec::new(),
            content: Some("a dog on a beach".into()),
            sort_by: SortBy::Name,
            descending: false,
        }
    }

    /// REQ-036-07: an album keeps a search's content words, and is named after them.
    #[test]
    fn req_036_07_keeps_the_content_words() {
        let search = GallerySearch {
            content: Some("dogs on a beach".into()),
            tags: vec!["Trip".into()],
            ..GallerySearch::default()
        };
        assert_eq!(search_to_value(&search)["content"], "dogs on a beach");
        assert_eq!(search_from_value(&search_to_value(&search)), search);
        assert_eq!(suggested_name(&search), "dogs on a beach, Trip");
    }

    #[test]
    fn req_034_07_reads_back_every_part_of_a_search() {
        let search = full();
        assert_eq!(search_from_value(&search_to_value(&search)), search);
        let default = GallerySearch::default();
        assert_eq!(search_to_value(&default), json!({}));
        assert_eq!(search_from_value(&json!({})), default);
        for sort_by in SortBy::ALL {
            let search = GallerySearch {
                sort_by,
                ..GallerySearch::default()
            };
            assert_eq!(search_from_value(&search_to_value(&search)), search);
        }
    }

    #[test]
    fn req_034_07_keeps_defaults_for_what_it_cant_read() {
        let odd = json!({"library": "three", "kind": "sound", "tags": ["Rome", 4], "from": "May", "ratings": [9, 4], "sort_by": "colour", "descending": "no"});
        let search = search_from_value(&odd);
        assert_eq!(search.library, None);
        assert_eq!(search.kind, None);
        assert_eq!(search.tags, ["Rome"]);
        assert_eq!(search.from, None);
        assert_eq!(search.ratings, [4]);
        assert_eq!(search.sort_by, SortBy::Taken);
        assert!(search.descending);
        assert_eq!(search_from_value(&json!("not a search")), GallerySearch::default());
    }

    #[test]
    fn req_034_01_suggests_a_name_from_tags_and_file_name() {
        assert_eq!(suggested_name(&full()), "a dog on a beach, Rome, Anna, -Blurry, IMG");
        let rated = GallerySearch {
            ratings: vec![5],
            ..GallerySearch::default()
        };
        assert_eq!(suggested_name(&rated), "Smart album");
    }

    #[test]
    fn req_034_01_req_034_05_trims_names_and_refuses_empty_ones() {
        assert_eq!(clean_name("  Rome 2023 "), Some("Rome 2023".into()));
        assert_eq!(clean_name("   "), None);
    }
}
