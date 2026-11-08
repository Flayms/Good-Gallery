//! A photo's versions (#247): the recipes it was saved with over its unedited original, oldest first, the last being
//! the one the file holds. Each is only a recipe, so keeping them costs no pixels; the file carries them in its XMP
//! (`gg:EditHistory`) and the index keeps a copy.

use crate::edit::Recipe;
use serde_json::{Value, json};

/// The most versions a file keeps; a save beyond drops the oldest (REQ-021-24).
pub const MAX_VERSIONS: usize = 20;

/// A saved edit: when it was saved, and its recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    /// Seconds since 1970 (UTC); `None` for an edit saved before versions were kept.
    pub saved_at: Option<i64>,
    pub recipe: Recipe,
}

/// Reads `gg:EditHistory`: a JSON list of `{"t": seconds, "r": recipe}`. What isn't readable is left out, and a list
/// longer than [`MAX_VERSIONS`] keeps its newest (REQ-011-07: the file may come from anywhere).
pub fn parse(history: &str) -> Vec<Version> {
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(history) else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = entries
        .iter()
        .filter_map(|entry| {
            let recipe = Recipe::from_json(&entry.get("r")?.to_string())?;
            let saved_at = entry.get("t").and_then(Value::as_i64);
            Some(Version { saved_at, recipe })
        })
        .collect();
    let extra = versions.len().saturating_sub(MAX_VERSIONS);
    versions.drain(..extra);
    versions
}

/// `versions` as stored in `gg:EditHistory`.
pub fn to_json(versions: &[Version]) -> String {
    let entries: Vec<Value> = versions
        .iter()
        .map(|version| {
            let recipe: Value = serde_json::from_str(&version.recipe.to_json()).unwrap_or(Value::Null);
            match version.saved_at {
                Some(t) => json!({ "t": t, "r": recipe }),
                None => json!({ "r": recipe }),
            }
        })
        .collect();
    Value::Array(entries).to_string()
}

/// The versions of a file saved with `current` and `history`: those of the history, ending with `current`. A file
/// saved before versions were kept, or whose history another program left behind, gets `current` as its last
/// version, without a date.
pub fn of_file(current: &Recipe, history: &str) -> Vec<Version> {
    let mut versions = parse(history);
    if versions.last().is_none_or(|last| last.recipe != *current) {
        versions.push(Version {
            saved_at: None,
            recipe: current.clone(),
        });
        let extra = versions.len().saturating_sub(MAX_VERSIONS);
        versions.drain(..extra);
    }
    versions
}

/// The versions after saving `recipe` at `now` over a photo with `earlier` (REQ-021-24): it becomes the last, and the
/// oldest go beyond [`MAX_VERSIONS`].
pub fn after_save(earlier: &[Version], recipe: &Recipe, now: i64) -> Vec<Version> {
    let mut versions = earlier.to_vec();
    versions.push(Version {
        saved_at: Some(now),
        recipe: recipe.clone(),
    });
    let extra = versions.len().saturating_sub(MAX_VERSIONS);
    versions.drain(..extra);
    versions
}

/// The versions after clearing the history (REQ-021-26): only the one the file holds.
pub fn cleared(versions: &[Version]) -> Vec<Version> {
    versions.last().cloned().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exposure(value: f32) -> Recipe {
        Recipe {
            exposure: value,
            ..Recipe::default()
        }
    }

    /// REQ-021-24: each save adds a version with its time; beyond the limit the oldest goes; the list round-trips.
    #[test]
    fn req_021_24_a_save_adds_a_version_and_the_oldest_go() {
        let first = after_save(&[], &exposure(1.0), 100);
        assert_eq!(
            first,
            vec![Version {
                saved_at: Some(100),
                recipe: exposure(1.0)
            }]
        );
        let second = after_save(&first, &exposure(2.0), 200);
        assert_eq!(second.len(), 2);
        assert_eq!(second[1].recipe.exposure, 2.0);
        assert_eq!(parse(&to_json(&second)), second);

        let mut many = Vec::new();
        for n in 0..MAX_VERSIONS + 3 {
            many = after_save(&many, &exposure(n as f32 / 10.0), n as i64);
        }
        assert_eq!(many.len(), MAX_VERSIONS);
        assert_eq!(many[0].saved_at, Some(3), "the oldest went");
        assert_eq!(many.last().unwrap().saved_at, Some(MAX_VERSIONS as i64 + 2));
    }

    /// REQ-021-24: a file saved before versions were kept has its edit as its only version; a history that doesn't
    /// end with the file's edit gets it added; unreadable entries and lists are left out.
    #[test]
    fn req_021_24_reads_what_a_file_holds() {
        assert_eq!(
            of_file(&exposure(1.0), ""),
            vec![Version {
                saved_at: None,
                recipe: exposure(1.0)
            }]
        );
        let history = to_json(&after_save(&[], &exposure(1.0), 100));
        assert_eq!(of_file(&exposure(1.0), &history).len(), 1);
        let other = of_file(&exposure(2.0), &history);
        assert_eq!(other.len(), 2);
        assert_eq!(
            other[1],
            Version {
                saved_at: None,
                recipe: exposure(2.0)
            }
        );

        assert!(parse("not json").is_empty());
        assert!(parse(r#"{"t": 1}"#).is_empty());
        let mixed = format!(
            r#"[{{"t": 5, "r": {}}}, {{"r": "nonsense"}}, 7]"#,
            exposure(1.0).to_json()
        );
        assert_eq!(
            parse(&mixed),
            vec![Version {
                saved_at: Some(5),
                recipe: exposure(1.0)
            }]
        );
        let long: Vec<Version> = (0..MAX_VERSIONS + 5)
            .map(|n| Version {
                saved_at: Some(n as i64),
                recipe: exposure(0.0),
            })
            .collect();
        assert_eq!(parse(&to_json(&long))[0].saved_at, Some(5));
    }

    /// REQ-021-26: clearing keeps only the version the file holds.
    #[test]
    fn req_021_26_clearing_keeps_the_current_version() {
        let versions = after_save(&after_save(&[], &exposure(1.0), 1), &exposure(2.0), 2);
        assert_eq!(cleared(&versions), vec![versions[1].clone()]);
        assert!(cleared(&[]).is_empty());
    }
}
