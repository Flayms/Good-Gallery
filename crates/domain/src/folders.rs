//! The folder tree of a library (REQ-008-02) and the tag trees of the People and Places groups (REQ-008-03).

use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderNode {
    pub name: String,
    /// `/`-separated, relative to the library.
    pub path: String,
    /// Media items in this folder and all its subfolders.
    pub count: u64,
    pub children: Vec<FolderNode>,
}

/// Letters without accents and letter case, as a base-sensitivity collator compares them (`Ä` like `a`).
fn base_letters(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;
    text.nfkd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// Natural order: digit runs compare by value (`2` before `10`), everything else ignoring letter case and accents
/// (the reference's `Intl.Collator` with `numeric` and base sensitivity); equal names fall back to code points.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_base_cmp(&base_letters(a), &base_letters(b)).then_with(|| a.cmp(b))
}

fn natural_base_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        chars.next();
                    }
                    digits
                };
                let (x, y) = (take(&mut a), take(&mut b));
                let (xs, ys) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let ordering = xs.len().cmp(&ys.len()).then_with(|| xs.cmp(ys));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(x), Some(y)) => {
                let ordering = x.to_lowercase().cmp(y.to_lowercase());
                if ordering != Ordering::Equal {
                    return ordering;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The tree of folders that directly contain media items (`path`, direct count), with the folders in between added,
/// counts summed up, and every level in natural order.
pub fn build_folder_tree(folders: &[(String, u64)]) -> Vec<FolderNode> {
    fn insert(nodes: &mut Vec<FolderNode>, segments: &[&str], prefix: &str, count: u64) {
        let Some((name, rest)) = segments.split_first() else {
            return;
        };
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let index = match nodes.iter().position(|node| node.name == *name) {
            Some(index) => index,
            None => {
                nodes.push(FolderNode {
                    name: name.to_string(),
                    path: path.clone(),
                    count: 0,
                    children: Vec::new(),
                });
                nodes.len() - 1
            }
        };
        nodes[index].count += count;
        insert(&mut nodes[index].children, rest, &path, count);
    }
    fn sort(nodes: &mut [FolderNode]) {
        nodes.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        for node in nodes {
            sort(&mut node.children);
        }
    }
    let mut roots = Vec::new();
    for (path, count) in folders {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        insert(&mut roots, &segments, "", *count);
    }
    sort(&mut roots);
    roots
}

/// `path` and all its ancestors: `a`, `a/b` for `a/b`.
pub fn folder_ancestors(path: &str) -> Vec<String> {
    let segments: Vec<&str> = path.split('/').collect();
    (1..=segments.len()).map(|n| segments[..n].join("/")).collect()
}

/// A tag of a category with its own place in the category's hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryTag {
    pub id: i64,
    pub name: String,
    /// `None` directly below the category root (the root itself isn't listed).
    pub parent_id: Option<i64>,
    /// Media items tagged with this tag or one of its descendants.
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagNode {
    pub id: i64,
    pub name: String,
    pub count: u64,
    pub children: Vec<TagNode>,
}

/// A category's flat tag list as a tree, keeping the given order at every level; tags whose parent isn't listed
/// go to the top level.
pub fn build_tag_tree(tags: &[CategoryTag]) -> Vec<TagNode> {
    let listed: HashMap<i64, usize> = tags.iter().enumerate().map(|(i, tag)| (tag.id, i)).collect();
    let mut parents: Vec<Option<usize>> = tags
        .iter()
        .map(|tag| tag.parent_id.and_then(|parent| listed.get(&parent).copied()))
        .collect();
    // The index tolerates parent cycles; a tag closing one goes to the top level, so no tag is lost.
    for start in 0..tags.len() {
        let mut seen = std::collections::HashSet::new();
        let mut at = start;
        while let Some(parent) = parents[at] {
            if !seen.insert(at) {
                parents[at] = None;
                break;
            }
            at = parent;
        }
    }
    let mut children: HashMap<Option<i64>, Vec<usize>> = HashMap::new();
    for (i, parent) in parents.iter().enumerate() {
        children.entry(parent.map(|p| tags[p].id)).or_default().push(i);
    }
    fn build(tags: &[CategoryTag], children: &HashMap<Option<i64>, Vec<usize>>, parent: Option<i64>) -> Vec<TagNode> {
        children
            .get(&parent)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&i| TagNode {
                        id: tags[i].id,
                        name: tags[i].name.clone(),
                        count: tags[i].count,
                        children: build(tags, children, Some(tags[i].id)),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    build(tags, &children, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_008_02_adds_intermediate_folders_sums_counts_and_sorts_naturally() {
        let tree = build_folder_tree(&[("2024/10".into(), 2), ("2024/9/party".into(), 3), ("2023".into(), 1)]);

        let top: Vec<(&str, u64)> = tree.iter().map(|n| (n.path.as_str(), n.count)).collect();
        assert_eq!(top, [("2023", 1), ("2024", 5)]);
        let months: Vec<(&str, u64)> = tree[1].children.iter().map(|n| (n.name.as_str(), n.count)).collect();
        assert_eq!(months, [("9", 3), ("10", 2)]);
        let party = &tree[1].children[0].children[0];
        assert_eq!(
            (party.path.as_str(), party.count, party.children.len()),
            ("2024/9/party", 3, 0)
        );
    }

    #[test]
    fn req_008_02_sorts_naturally_ignoring_letter_case() {
        let mut names = ["b", "A", "img10", "img2", "Img1", "a"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names[..2].iter().map(|s| s.to_lowercase()).collect::<Vec<_>>(),
            ["a", "a"]
        );
        assert_eq!(&names[2..], ["b", "Img1", "img2", "img10"]);
    }

    #[test]
    fn req_008_02_sorts_accented_names_with_their_base_letters() {
        let mut names = ["Zürich", "Bali", "Ägypten", "École", "Florenz"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["Ägypten", "Bali", "École", "Florenz", "Zürich"]);
        // The accent counts for nothing: Äa before Ab.
        let mut pair = ["Ab", "Äa"];
        pair.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(pair, ["Äa", "Ab"]);
    }

    #[test]
    fn req_008_03_keeps_tags_whose_parents_form_a_cycle() {
        let tree = build_tag_tree(&[
            tag(1, "A", Some(2), 1),
            tag(2, "B", Some(1), 1),
            tag(3, "C", Some(1), 1),
        ]);
        fn count(nodes: &[TagNode]) -> usize {
            nodes.iter().map(|node| 1 + count(&node.children)).sum()
        }
        assert_eq!(count(&tree), 3);
    }

    #[test]
    fn req_008_02_lists_the_path_and_its_ancestors() {
        assert_eq!(folder_ancestors("a/b/c"), ["a", "a/b", "a/b/c"]);
    }

    fn tag(id: i64, name: &str, parent_id: Option<i64>, count: u64) -> CategoryTag {
        CategoryTag {
            id,
            name: name.into(),
            parent_id,
            count,
        }
    }

    #[test]
    fn req_008_03_nests_tags_under_their_parent_in_the_given_order() {
        let flat = build_tag_tree(&[tag(1, "Anton", None, 3), tag(2, "Bea", None, 1)]);
        let names: Vec<(&str, u64, usize)> = flat
            .iter()
            .map(|n| (n.name.as_str(), n.count, n.children.len()))
            .collect();
        assert_eq!(names, [("Anton", 3, 0), ("Bea", 1, 0)]);

        let nested = build_tag_tree(&[tag(1, "France", None, 2), tag(2, "Paris", Some(1), 2)]);
        assert_eq!(
            nested,
            [TagNode {
                id: 1,
                name: "France".into(),
                count: 2,
                children: vec![TagNode {
                    id: 2,
                    name: "Paris".into(),
                    count: 2,
                    children: vec![]
                }],
            }]
        );
    }
}
