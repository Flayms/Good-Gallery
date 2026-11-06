//! Renaming many files (spec 035): the pattern, the new names, what makes a name unusable, and the order of the
//! renames, through temporary names where one file takes another's name.

use crate::time_zone;
use regex::{NoExpand, Regex, RegexBuilder};
use std::collections::HashSet;

/// What the user asked for in the rename dialog (REQ-035-02).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pattern {
    pub find: String,
    pub replace: String,
    pub regex: bool,
    pub match_case: bool,
}

/// A pattern ready to apply; `find` is `None` for an empty search, which replaces the whole name.
#[derive(Debug, Clone)]
pub struct Compiled {
    find: Option<Regex>,
    replace: String,
    regex: bool,
}

impl Pattern {
    /// The pattern ready to apply, or why a regular expression can't be used.
    pub fn compile(&self) -> Result<Compiled, String> {
        let find = if self.find.is_empty() {
            None
        } else {
            let source = if self.regex {
                self.find.clone()
            } else {
                regex::escape(&self.find)
            };
            let regex = RegexBuilder::new(&source)
                .case_insensitive(!self.match_case)
                .size_limit(1 << 20)
                .build()
                .map_err(|error| error.to_string())?;
            Some(regex)
        };
        Ok(Compiled {
            find,
            replace: self.replace.clone(),
            regex: self.regex,
        })
    }
}

/// What a media item contributes to its new name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: i64,
    pub rel_path: String,
    /// Milliseconds since the Unix epoch.
    pub taken_at: Option<i64>,
    pub mtime: i64,
    /// Tags outside People and Places, in the file's order (REQ-035-03).
    pub tags: Vec<String>,
}

/// Why a row can't be renamed (REQ-035-03, REQ-035-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A placeholder the media item has no value for.
    MissingValue(&'static str),
    Empty,
    Forbidden(char),
    EndsWith(char),
    Reserved(String),
    TooLong,
    /// Another file of the batch gets, or keeps, the same name.
    Duplicate,
    /// A file of that name is already there.
    Exists,
}

impl Problem {
    /// What the preview says.
    pub fn text(&self) -> String {
        match self {
            Self::MissingValue(what) => format!("Has no {what}"),
            Self::Empty => "The name would be empty".into(),
            Self::Forbidden(c) if c.is_control() => "Contains a control character".into(),
            Self::Forbidden(c) => format!("Contains {c}, which Windows doesn't allow"),
            Self::EndsWith(' ') => "Ends with a space".into(),
            Self::EndsWith(c) => format!("Ends with {c}"),
            Self::Reserved(name) => format!("{name} is reserved by Windows"),
            Self::TooLong => "The path would be too long for Windows".into(),
            Self::Duplicate => "Another file gets this name too".into(),
            Self::Exists => "A file of this name exists".into(),
        }
    }
}

/// What happens to a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Unchanged,
    /// The new path, relative to the library.
    Renamed(String),
    Failed(Problem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: i64,
    pub old: String,
    pub outcome: Outcome,
}

/// The longest relative path that keeps a library at `library_path` within Windows' 259 characters (REQ-035-05).
pub fn max_rel_len(library_path: &str) -> usize {
    259usize.saturating_sub(library_path.chars().count() + 1)
}

fn split_name(rel_path: &str) -> (&str, &str) {
    let name_start = rel_path.rfind('/').map_or(0, |slash| slash + 1);
    (&rel_path[..name_start], &rel_path[name_start..])
}

/// A file name's stem and its extension with the dot (`""` without one).
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    }
}

/// `yyyy-MM-dd HH-mm-ss` and its parts as a `chrono` format; everything else is literal.
fn date_format(format: &str) -> String {
    let mut out = String::new();
    let mut rest = format;
    while !rest.is_empty() {
        let token = ["yyyy", "yy", "MM", "dd", "HH", "mm", "ss"]
            .into_iter()
            .find(|token| rest.starts_with(token));
        match token {
            Some(token) => {
                out.push_str(match token {
                    "yyyy" => "%Y",
                    "yy" => "%y",
                    "MM" => "%m",
                    "dd" => "%d",
                    "HH" => "%H",
                    "mm" => "%M",
                    _ => "%S",
                });
                rest = &rest[token.len()..];
            }
            None => {
                let c = rest.chars().next().expect("not empty");
                if c == '%' {
                    out.push_str("%%");
                } else {
                    out.push(c);
                }
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// The replacement with its placeholders filled in for `item` (REQ-035-03).
/// With `escape`, `$` in values is doubled, so a regular expression's replacement takes it as it is.
fn expand(replace: &str, stem: &str, item: &Item, escape: bool) -> Result<String, Problem> {
    let mut out = String::new();
    let mut rest = replace;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return Ok(out);
        };
        let inside = &after[..close];
        let (name, format) = inside.split_once(':').unwrap_or((inside, "yyyy-MM-dd"));
        let value = match name {
            "name" => Some(stem.to_string()),
            "tags" if item.tags.is_empty() => return Err(Problem::MissingValue("tags")),
            "tags" => Some(item.tags.join(", ")),
            "taken" => Some(
                item.taken_at
                    .and_then(time_zone::wall_clock)
                    .ok_or(Problem::MissingValue("date taken"))?
                    .format(&date_format(format))
                    .to_string(),
            ),
            "modified" => time_zone::wall_clock(item.mtime).map(|at| at.format(&date_format(format)).to_string()),
            _ => None,
        };
        match value {
            Some(value) if escape => out.push_str(&value.replace('$', "$$")),
            Some(value) => out.push_str(&value),
            // Not a placeholder: kept as typed.
            None => out.push_str(&rest[open..=open + 1 + close]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

const FORBIDDEN: &str = "<>:\"/\\|?*";
const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

/// Why `name` can't be a file name on Windows, if it can't (REQ-035-05).
fn check_name(name: &str) -> Result<(), Problem> {
    let (stem, _) = split_ext(name);
    if stem.trim().is_empty() {
        return Err(Problem::Empty);
    }
    if let Some(c) = name.chars().find(|&c| FORBIDDEN.contains(c) || c.is_control()) {
        return Err(Problem::Forbidden(c));
    }
    if let Some(c) = name.chars().last().filter(|&c| c == ' ' || c == '.') {
        return Err(Problem::EndsWith(c));
    }
    let base = name.split('.').next().unwrap_or_default().trim_end().to_uppercase();
    let numbered = (base.starts_with("COM") || base.starts_with("LPT"))
        && base.len() == 4
        && base.chars().nth(3).is_some_and(|c| ('1'..='9').contains(&c));
    if RESERVED.contains(&base.as_str()) || numbered {
        return Err(Problem::Reserved(base));
    }
    Ok(())
}

/// The new file name of `item`, extension unchanged (REQ-035-02, REQ-035-03).
pub fn new_name(compiled: &Compiled, item: &Item) -> Result<String, Problem> {
    let (_, name) = split_name(&item.rel_path);
    let (stem, ext) = split_ext(name);
    let regex = compiled.regex && compiled.find.is_some();
    let replacement = expand(&compiled.replace, stem, item, regex)?;
    let new_stem = match &compiled.find {
        // Nothing asked for yet: every name stays.
        None if compiled.replace.is_empty() => stem.to_string(),
        None => replacement,
        Some(find) if compiled.regex => find.replace_all(stem, replacement.as_str()).into_owned(),
        Some(find) => find.replace_all(stem, NoExpand(&replacement)).into_owned(),
    };
    if new_stem.trim().is_empty() {
        return Err(Problem::Empty);
    }
    let new = format!("{new_stem}{ext}");
    check_name(&new)?;
    Ok(new)
}

/// Every item with its new name, or why it keeps its name (REQ-035-04, REQ-035-05). `exists` says whether a file of
/// that relative path is on disk; paths longer than `max_rel_len` characters fail.
pub fn preview(compiled: &Compiled, items: &[Item], max_rel_len: usize, exists: impl Fn(&str) -> bool) -> Vec<Row> {
    let mut rows: Vec<Row> = items
        .iter()
        .map(|item| {
            let (folder, name) = split_name(&item.rel_path);
            let outcome = match new_name(compiled, item) {
                Ok(new) if new == name => Outcome::Unchanged,
                Ok(new) => {
                    let path = format!("{folder}{new}");
                    if path.chars().count() > max_rel_len {
                        Outcome::Failed(Problem::TooLong)
                    } else {
                        Outcome::Renamed(path)
                    }
                }
                Err(problem) => Outcome::Failed(problem),
            };
            Row {
                id: item.id,
                old: item.rel_path.clone(),
                outcome,
            }
        })
        .collect();
    // Compared without case: Windows, macOS and most shares don't tell `a.jpg` from `A.jpg`. A failed row keeps its
    // name, which can fail another, so this runs until nothing changes.
    let key = |path: &str| path.to_lowercase();
    loop {
        let mut targets: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut leaving: HashSet<String> = HashSet::new();
        for row in &rows {
            match &row.outcome {
                Outcome::Renamed(new) => {
                    *targets.entry(key(new)).or_default() += 1;
                    leaving.insert(key(&row.old));
                }
                _ => *targets.entry(key(&row.old)).or_default() += 1,
            }
        }
        let mut changed = false;
        for row in &mut rows {
            let Outcome::Renamed(new) = &row.outcome else { continue };
            let new_key = key(new);
            let problem = if targets[&new_key] > 1 {
                Some(Problem::Duplicate)
            } else if new_key != key(&row.old) && exists(new) && !leaving.contains(&new_key) {
                Some(Problem::Exists)
            } else {
                None
            };
            if let Some(problem) = problem {
                row.outcome = Outcome::Failed(problem);
                changed = true;
            }
        }
        if !changed {
            return rows;
        }
    }
}

/// One rename of a file, relative to its library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The rename (index into the list given to [`steps`]) this is part of.
    pub row: usize,
    pub from: String,
    pub to: String,
}

/// Where a file waits while another takes its name: hidden beside it.
pub fn temp_path(rel_path: &str) -> String {
    let (folder, name) = split_name(rel_path);
    format!("{folder}.{name}.gg-rename")
}

/// The renames `old → new` in an order that works: a file whose new name is still another's (or its own, in another
/// case) goes through a temporary name first (REQ-035-06).
pub fn steps(renames: &[(String, String)]) -> Vec<Step> {
    let taken: HashSet<String> = renames.iter().map(|(old, _)| old.to_lowercase()).collect();
    let (mut first, mut direct, mut last) = (Vec::new(), Vec::new(), Vec::new());
    for (row, (old, new)) in renames.iter().enumerate() {
        if taken.contains(&new.to_lowercase()) {
            let temp = temp_path(old);
            first.push(Step {
                row,
                from: old.clone(),
                to: temp.clone(),
            });
            last.push(Step {
                row,
                from: temp,
                to: new.clone(),
            });
        } else {
            direct.push(Step {
                row,
                from: old.clone(),
                to: new.clone(),
            });
        }
    }
    first.extend(direct);
    first.extend(last);
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, rel_path: &str) -> Item {
        Item {
            id,
            rel_path: rel_path.into(),
            // 2023-05-12 10:15:00 UTC.
            taken_at: Some(1_683_886_500_000),
            mtime: 1_700_000_000_000,
            tags: vec!["Beach".into(), "Summer".into()],
        }
    }

    fn pattern(find: &str, replace: &str, regex: bool, match_case: bool) -> Compiled {
        Pattern {
            find: find.into(),
            replace: replace.into(),
            regex,
            match_case,
        }
        .compile()
        .unwrap()
    }

    fn utc() {
        time_zone::set_fixed(chrono::FixedOffset::east_opt(0));
    }

    #[test]
    fn req_035_02_replaces_plain_text_with_or_without_case_and_keeps_the_extension() {
        let photo = item(1, "trip/IMG_img_1.JPG");
        assert_eq!(
            new_name(&pattern("img", "photo", false, false), &photo).unwrap(),
            "photo_photo_1.JPG"
        );
        assert_eq!(
            new_name(&pattern("img", "photo", false, true), &photo).unwrap(),
            "IMG_photo_1.JPG"
        );
        // Plain text is taken as it is, also characters a regular expression would read.
        let dots = item(2, "a.b.c.jpg");
        assert_eq!(new_name(&pattern(".", "-", false, true), &dots).unwrap(), "a-b-c.jpg");
        assert_eq!(
            new_name(&pattern("$", "x", false, true), &item(3, "a$.jpg")).unwrap(),
            "ax.jpg"
        );
    }

    #[test]
    fn req_035_02_uses_groups_of_a_regular_expression_and_an_empty_search_replaces_the_name() {
        let photo = item(1, "PXL_20230512_101500123.jpg");
        let swapped = pattern(r"PXL_(\d{8})_(\d{6})\d*", "$2 $1", true, true);
        assert_eq!(new_name(&swapped, &photo).unwrap(), "101500 20230512.jpg");
        assert_eq!(
            new_name(&pattern("", "holiday", false, false), &photo).unwrap(),
            "holiday.jpg"
        );
        // Nothing typed yet changes nothing.
        assert_eq!(
            new_name(&pattern("", "", false, false), &photo).unwrap(),
            "PXL_20230512_101500123.jpg"
        );
        assert!(
            Pattern {
                find: "(".into(),
                regex: true,
                ..Pattern::default()
            }
            .compile()
            .is_err()
        );
    }

    #[test]
    fn req_035_03_fills_in_placeholders() {
        utc();
        let photo = item(1, "IMG_1.jpg");
        let named = |replace: &str| new_name(&pattern("", replace, false, false), &photo);
        assert_eq!(named("{taken} {name}").unwrap(), "2023-05-12 IMG_1.jpg");
        assert_eq!(named("{taken:yyyyMMdd_HHmmss}").unwrap(), "20230512_101500.jpg");
        assert_eq!(named("{modified:yy-MM}").unwrap(), "23-11.jpg");
        assert_eq!(named("{tags} - {name}").unwrap(), "Beach, Summer - IMG_1.jpg");
        // Braces that aren't a placeholder stay, and a value isn't read as a group.
        assert_eq!(named("{other} {name").unwrap(), "{other} {name.jpg");
        let dollars = Item {
            tags: vec!["$1".into()],
            ..photo.clone()
        };
        assert_eq!(
            new_name(&pattern("IMG", "{tags}", true, true), &dollars).unwrap(),
            "$1_1.jpg"
        );
        let bare = Item {
            taken_at: None,
            tags: Vec::new(),
            ..photo
        };
        let named = |replace: &str| new_name(&pattern("", replace, false, false), &bare);
        assert_eq!(named("{taken}"), Err(Problem::MissingValue("date taken")));
        assert_eq!(named("{tags}"), Err(Problem::MissingValue("tags")));
    }

    #[test]
    fn req_035_05_refuses_names_windows_cant_use() {
        let named = |replace: &str| new_name(&pattern("", replace, false, false), &item(1, "a.jpg"));
        assert_eq!(
            new_name(&pattern("a", "", false, false), &item(1, "a.jpg")),
            Err(Problem::Empty)
        );
        assert_eq!(named("   "), Err(Problem::Empty));
        assert_eq!(named("a:b"), Err(Problem::Forbidden(':')));
        assert_eq!(named("a/b"), Err(Problem::Forbidden('/')));
        assert_eq!(named("a\tb"), Err(Problem::Forbidden('\t')));
        assert_eq!(named("con"), Err(Problem::Reserved("CON".into())));
        assert_eq!(named("lpt3"), Err(Problem::Reserved("LPT3".into())));
        assert!(named("console").is_ok() && named("COM10").is_ok());
        let no_ext = item(2, "README");
        assert_eq!(
            new_name(&pattern("", "x.", false, false), &no_ext),
            Err(Problem::EndsWith('.'))
        );
        assert_eq!(
            new_name(&pattern("", "x ", false, false), &no_ext),
            Err(Problem::EndsWith(' '))
        );
        assert_eq!(max_rel_len("C:\\Photos"), 249);
    }

    fn outcomes(rows: &[Row]) -> Vec<(&str, Outcome)> {
        rows.iter().map(|row| (row.old.as_str(), row.outcome.clone())).collect()
    }

    #[test]
    fn req_035_04_req_035_05_previews_every_row_and_leaves_out_the_ones_that_cant_work() {
        let items = [
            item(1, "a/IMG_1.jpg"),
            item(2, "a/IMG_2.jpg"),
            item(3, "a/IMG_3.jpg"),
            item(4, "b/IMG_3.jpg"),
            item(5, "a/other.jpg"),
        ];
        // _1 and _2 both become `x`; _3 in `a` meets a file on disk; in `b` it is free.
        let compiled = pattern(r"IMG_[12]", "x", true, true);
        let on_disk = |rel: &str| rel == "a/x3.jpg";
        let rows = preview(&pattern("IMG_", "x", false, true), &items[2..], 260, on_disk);
        assert_eq!(
            outcomes(&rows),
            [
                ("a/IMG_3.jpg", Outcome::Failed(Problem::Exists)),
                ("b/IMG_3.jpg", Outcome::Renamed("b/x3.jpg".into())),
                ("a/other.jpg", Outcome::Unchanged),
            ]
        );
        let rows = preview(&compiled, &items, 260, |_| false);
        assert_eq!(rows[0].outcome, Outcome::Failed(Problem::Duplicate));
        assert_eq!(rows[1].outcome, Outcome::Failed(Problem::Duplicate));
        assert_eq!(rows[2].outcome, Outcome::Unchanged);
        // Too long for the library's place.
        let rows = preview(
            &pattern("", "{name}{name}{name}", false, false),
            &items[..1],
            20,
            |_| false,
        );
        assert_eq!(rows[0].outcome, Outcome::Failed(Problem::TooLong));
    }

    #[test]
    fn req_035_05_names_freed_in_the_batch_can_be_taken_and_names_kept_cant() {
        let items = [item(1, "a.jpg"), item(2, "b.jpg")];
        let on_disk = |rel: &str| rel == "a.jpg" || rel == "b.jpg";
        // b.jpg isn't in the batch, so it stays where it is.
        let rows = preview(&pattern("a", "b", false, true), &items[..1], 260, on_disk);
        assert_eq!(rows[0].outcome, Outcome::Failed(Problem::Exists));
        // Both move on: no name is taken.
        let rows = preview(&pattern("^(a|b)$", "${1}2", true, true), &items, 260, on_disk);
        assert_eq!(
            outcomes(&rows),
            [
                ("a.jpg", Outcome::Renamed("a2.jpg".into())),
                ("b.jpg", Outcome::Renamed("b2.jpg".into())),
            ]
        );
        // b → a while a → c: a's name is free once it moved.
        let chain = [item(1, "a.jpg"), item(2, "b.jpg")];
        let compiled = pattern("^a$", "c", true, true);
        let rows = preview(&compiled, &chain, 260, on_disk);
        assert_eq!(rows[0].outcome, Outcome::Renamed("c.jpg".into()));
        // A change in case only is the file's own name.
        let rows = preview(&pattern("a", "A", false, true), &chain[..1], 260, on_disk);
        assert_eq!(rows[0].outcome, Outcome::Renamed("A.jpg".into()));
    }

    /// REQ-035-04: the preview of 50,000 items is worked out again on every key press, so it must stay quick.
    #[test]
    fn req_035_04_previews_50000_items_quickly() {
        utc();
        let items: Vec<Item> = (0..50_000).map(|i| item(i, &format!("2023/IMG_{i:05}.jpg"))).collect();
        let compiled = pattern(r"IMG_(\d+)", "{taken:yyyy-MM-dd} $1", true, true);
        let started = std::time::Instant::now();
        let rows = preview(&compiled, &items, 260, |_| false);
        let took = started.elapsed();
        assert!(rows.iter().all(|row| matches!(row.outcome, Outcome::Renamed(_))));
        eprintln!("50,000 rows in {took:?}");
        // Generous for a debug build on a slow machine; a release build takes a fraction of it.
        assert!(took < std::time::Duration::from_secs(3), "{took:?}");
    }

    #[test]
    fn req_035_06_orders_swaps_chains_and_case_changes_through_temporary_names() {
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
        };
        let as_tuples =
            |steps: Vec<Step>| -> Vec<(String, String)> { steps.into_iter().map(|s| (s.from, s.to)).collect() };
        assert_eq!(
            as_tuples(steps(&pairs(&[("d/a.jpg", "d/b.jpg"), ("d/b.jpg", "d/a.jpg")]))),
            pairs(&[
                ("d/a.jpg", "d/.a.jpg.gg-rename"),
                ("d/b.jpg", "d/.b.jpg.gg-rename"),
                ("d/.a.jpg.gg-rename", "d/b.jpg"),
                ("d/.b.jpg.gg-rename", "d/a.jpg"),
            ])
        );
        // a → c directly, b → a once a moved; a case change goes through its temporary name.
        assert_eq!(
            as_tuples(steps(&pairs(&[
                ("a.jpg", "c.jpg"),
                ("b.jpg", "a.jpg"),
                ("x.jpg", "X.jpg")
            ]))),
            pairs(&[
                ("b.jpg", ".b.jpg.gg-rename"),
                ("x.jpg", ".x.jpg.gg-rename"),
                ("a.jpg", "c.jpg"),
                (".b.jpg.gg-rename", "a.jpg"),
                (".x.jpg.gg-rename", "X.jpg"),
            ])
        );
    }
}
