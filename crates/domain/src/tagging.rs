//! Adding and removing a media item's tags by hand (spec 026): the new contents of a file's keyword fields.

use crate::tags::normalize_tag;

/// A tag change by hand: add the tag with this path (outermost first), or remove the tag with this name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add(Vec<String>),
    Remove(String),
}

/// The keyword fields of one file (a media file or its sidecar), as indexing reads them (REQ-004-06, REQ-004-07,
/// REQ-004-09).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keywords {
    /// `XMP-lr:HierarchicalSubject`: paths such as `Places|Italy|Rome`.
    pub hierarchical: Vec<String>,
    /// `XMP-dc:Subject`.
    pub subject: Vec<String>,
    /// `IPTC:Keywords`.
    pub iptc: Vec<String>,
    /// `XMP-pdf:Keywords`, which exiftool makes of IPTC keywords in a sidecar and indexing reads as `Keywords` too.
    pub pdf: Vec<String>,
    /// `IFD0:XPKeywords`, `;`-separated (Windows).
    pub xp: Option<String>,
    /// `XMP-iptcExt:PersonInImage`.
    pub people: Vec<String>,
}

impl Keywords {
    /// Whether the file has keywords of its own, so an added tag goes into it rather than a sidecar (REQ-026-03).
    pub fn is_empty(&self) -> bool {
        self.hierarchical.is_empty()
            && self.subject.is_empty()
            && self.iptc.is_empty()
            && self.pdf.is_empty()
            && self.xp.as_deref().is_none_or(str::is_empty)
            && self.people.is_empty()
    }
}

/// `keywords` with the tag `path` (outermost first, e.g. `["Places", "Italy", "Rome"]`) added: as a hierarchy, as its
/// name, and as an IPTC keyword if the file has those. Nothing is added twice (REQ-026-03).
pub fn add(keywords: &Keywords, path: &[String]) -> Keywords {
    let mut added = keywords.clone();
    let Some(name) = path.last() else { return added };
    let joined = path.join("|");
    push_new(&mut added.hierarchical, &joined, &normalize_path(&joined));
    push_new(&mut added.subject, name, &normalize_tag(name));
    if !added.iptc.is_empty() {
        push_new(&mut added.iptc, name, &normalize_tag(name));
    }
    added
}

/// `keywords` without the tag `name`: out of every field, with the levels of its hierarchies that no other hierarchy
/// still uses (REQ-026-03).
pub fn remove(keywords: &Keywords, name: &str) -> Keywords {
    let wanted = normalize_tag(name);
    let leaf = |path: &str| normalize_tag(path.rsplit('|').next().unwrap_or(path));
    let (gone, hierarchical): (Vec<String>, Vec<String>) = keywords
        .hierarchical
        .iter()
        .cloned()
        .partition(|path| leaf(path) == wanted);
    // Levels of the removed hierarchies, unless a remaining one still has them.
    let still_used: std::collections::HashSet<String> = hierarchical
        .iter()
        .flat_map(|path| path.split('|'))
        .map(normalize_tag)
        .collect();
    let mut dropped: std::collections::HashSet<String> = gone
        .iter()
        .flat_map(|path| path.split('|'))
        .map(normalize_tag)
        .filter(|level| !still_used.contains(level))
        .collect();
    dropped.insert(wanted.clone());
    let keep = |values: &[String]| -> Vec<String> {
        values
            .iter()
            .filter(|value| !dropped.contains(&normalize_tag(value)))
            .cloned()
            .collect()
    };
    let xp = keywords.xp.as_deref().and_then(|xp| {
        let left: Vec<&str> = xp
            .split(';')
            .filter(|value| !value.trim().is_empty() && !dropped.contains(&normalize_tag(value)))
            .collect();
        (!left.is_empty()).then(|| left.join(";"))
    });
    Keywords {
        hierarchical,
        subject: keep(&keywords.subject),
        iptc: keep(&keywords.iptc),
        pdf: keep(&keywords.pdf),
        xp,
        people: keywords
            .people
            .iter()
            .filter(|person| normalize_tag(person) != wanted)
            .cloned()
            .collect(),
    }
}

fn normalize_path(path: &str) -> String {
    path.split('|').map(normalize_tag).collect::<Vec<_>>().join("|")
}

/// Appends `value` unless `values` has it already, compared as `key` (normalized).
fn push_new(values: &mut Vec<String>, value: &str, key: &str) {
    let normalize = |other: &String| {
        if value.contains('|') {
            normalize_path(other)
        } else {
            normalize_tag(other)
        }
    };
    if !values.iter().any(|other| normalize(other) == key) {
        values.push(value.to_owned());
    }
}

/// Every tag of a category tree as a path under `root` (`["Places", "Italy", "Rome"]`), parents before children.
pub fn paths(root: &str, nodes: &[crate::folders::TagNode]) -> Vec<Vec<String>> {
    fn walk(prefix: &[String], nodes: &[crate::folders::TagNode], out: &mut Vec<Vec<String>>) {
        for node in nodes {
            let mut path = prefix.to_vec();
            path.push(node.name.clone());
            out.push(path.clone());
            walk(&path, &node.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&[root.to_owned()], nodes, &mut out);
    out
}

/// Up to `limit` of `paths` whose name contains `typed` (normalized), those starting with it first, leaving out the
/// names in `has` (REQ-026-01).
pub fn suggest<'a>(paths: &'a [Vec<String>], typed: &str, has: &[String], limit: usize) -> Vec<&'a Vec<String>> {
    let typed = normalize_tag(typed);
    let has: Vec<String> = has.iter().map(|name| normalize_tag(name)).collect();
    let mut found: Vec<(bool, &Vec<String>)> = paths
        .iter()
        .filter_map(|path| {
            let name = normalize_tag(path.last()?);
            (name.contains(&typed) && !has.contains(&name)).then_some((!name.starts_with(&typed), path))
        })
        .collect();
    found.sort_by_key(|(later, _)| *later);
    found.into_iter().take(limit).map(|(_, path)| path).collect()
}

/// The tag `typed` under `root` makes: an existing tag of the same name with its path, else a new one directly under
/// `root` (or on its own without one) (REQ-026-01).
pub fn path_for(paths: &[Vec<String>], root: Option<&str>, typed: &str) -> Vec<String> {
    let name = crate::tags::clean_tag(typed);
    let key = normalize_tag(&name);
    if let Some(path) = paths
        .iter()
        .find(|path| path.last().is_some_and(|last| normalize_tag(last) == key))
    {
        return path.clone();
    }
    root.map(str::to_owned).into_iter().chain([name]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folders::TagNode;

    /// REQ-026-01: suggestions are the group's tags containing what was typed, starting ones first, except those
    /// the item has; a typed name becomes the existing tag or a new one under the group.
    #[test]
    fn req_026_01_suggests_and_finds_the_tags_of_a_group() {
        let node = |name: &str, children: Vec<TagNode>| TagNode {
            id: 0,
            name: name.into(),
            count: 1,
            children,
        };
        let places = paths(
            "Places",
            &[
                node("Italy", vec![node("Rome", vec![]), node("Milan", vec![])]),
                node("Romania", vec![]),
            ],
        );
        assert_eq!(
            places,
            [
                strings(&["Places", "Italy"]),
                strings(&["Places", "Italy", "Rome"]),
                strings(&["Places", "Italy", "Milan"]),
                strings(&["Places", "Romania"]),
            ]
        );
        let names = |found: Vec<&Vec<String>>| {
            found
                .iter()
                .map(|path| path.last().unwrap().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(suggest(&places, "rom", &[], 5)), ["Rome", "Romania"]);
        assert_eq!(names(suggest(&places, "AN", &[], 5)), ["Milan", "Romania"]);
        assert_eq!(names(suggest(&places, "rom", &strings(&["Rome"]), 5)), ["Romania"]);
        assert_eq!(names(suggest(&places, "", &[], 2)), ["Italy", "Rome"]);

        assert_eq!(
            path_for(&places, Some("Places"), " rome "),
            strings(&["Places", "Italy", "Rome"])
        );
        assert_eq!(
            path_for(&places, Some("Places"), "Paris"),
            strings(&["Places", "Paris"])
        );
        assert_eq!(path_for(&[], None, "Beach"), strings(&["Beach"]));
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// REQ-026-03: an added tag is written as its hierarchy and its name, into IPTC only if the file uses it, once.
    #[test]
    fn req_026_03_adds_the_hierarchy_and_the_name() {
        let empty = Keywords::default();
        let rome = strings(&["Places", "Italy", "Rome"]);
        let added = add(&empty, &rome);
        assert_eq!(
            added,
            Keywords {
                hierarchical: strings(&["Places|Italy|Rome"]),
                subject: strings(&["Rome"]),
                ..Default::default()
            }
        );
        assert_eq!(add(&added, &strings(&["places", "italy", "ROME"])), added, "not twice");

        let iptc = Keywords {
            iptc: strings(&["Beach"]),
            ..Default::default()
        };
        let added = add(&iptc, &strings(&["People", "Anna"]));
        assert_eq!(added.iptc, strings(&["Beach", "Anna"]));
        assert_eq!(added.hierarchical, strings(&["People|Anna"]));
        assert!(!added.is_empty() && Keywords::default().is_empty());
    }

    /// REQ-026-03: a removed tag leaves every field, with the levels of its hierarchy no other hierarchy uses.
    #[test]
    fn req_026_03_removes_the_tag_from_every_field() {
        let keywords = Keywords {
            hierarchical: strings(&["Places|Italy|Rome", "Places|Italy|Milan", "People|Anna"]),
            subject: strings(&["Places", "Italy", "Rome", "Milan", "Anna", "Beach"]),
            iptc: strings(&["rome", "Beach"]),
            pdf: strings(&["Rome"]),
            xp: Some("Beach;Rome".into()),
            people: strings(&["Anna"]),
        };
        let removed = remove(&keywords, "Rome");
        assert_eq!(
            removed,
            Keywords {
                hierarchical: strings(&["Places|Italy|Milan", "People|Anna"]),
                subject: strings(&["Places", "Italy", "Milan", "Anna", "Beach"]),
                iptc: strings(&["Beach"]),
                pdf: Vec::new(),
                xp: Some("Beach".into()),
                people: strings(&["Anna"]),
            },
            "Places and Italy stay: Milan still uses them"
        );

        let removed = remove(&removed, "milan");
        assert_eq!(removed.hierarchical, strings(&["People|Anna"]));
        assert_eq!(
            removed.subject,
            strings(&["Anna", "Beach"]),
            "its levels go with the last hierarchy"
        );

        let removed = remove(&removed, "Anna");
        assert_eq!(removed.people, Vec::<String>::new());
        assert_eq!(removed.subject, strings(&["Beach"]));
        let removed = remove(&removed, "Beach");
        assert_eq!(removed.xp, None, "an emptied Windows field is deleted");
        assert!(removed.is_empty());
    }
}
