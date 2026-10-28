//! Tag names: normalization (glossary: _Tag_) and categories (glossary: _Tag category_).

use unicode_normalization::UnicodeNormalization;

/// Display form of a tag (`tags.name`): Unicode NFKC, trimmed, whitespace runs collapsed to one space.
pub fn clean_tag(tag: &str) -> String {
    let normalized: String = tag.nfkc().collect();
    normalized
        .split(is_js_space)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// JavaScript's whitespace (`\s`, `trim()`), which tag names in existing indexes were normalized with. It differs from
/// Rust's `char::is_whitespace`: it includes U+FEFF (byte order mark) and excludes U+0085 (next line).
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// Canonical form for lookup and uniqueness (`tags.name_norm`): [`clean_tag`], lower-cased.
pub fn normalize_tag(tag: &str) -> String {
    clean_tag(tag).to_lowercase()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagCategory {
    People,
    Places,
}

/// Top-level tag name the indexer uses for face-region names.
pub const PEOPLE_TAG_ROOT: &str = "People";

/// Normalized top-level names that make a tag hierarchy a category, whichever app wrote it.
pub const CATEGORY_ROOTS: [(TagCategory, &[&str]); 2] = [
    (TagCategory::People, &["people", "person", "persons", "faces"]),
    (TagCategory::Places, &["places", "place", "locations", "location"]),
];

/// The category a top-level tag name stands for, if any.
pub fn category_of_root(name: &str) -> Option<TagCategory> {
    let name = normalize_tag(name);
    CATEGORY_ROOTS
        .iter()
        .find(|(_, roots)| roots.contains(&name.as_str()))
        .map(|(category, _)| *category)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_004_11_trims_collapses_whitespace_and_case_folds() {
        assert_eq!(normalize_tag("  Summer   Holidays "), "summer holidays");
        assert_eq!(clean_tag("  Summer \t Holidays\n"), "Summer Holidays");
    }

    #[test]
    fn req_004_11_applies_nfkc_so_composed_and_decomposed_forms_match() {
        assert_eq!(normalize_tag("Cafe\u{301}"), normalize_tag("Café"));
        assert_eq!(normalize_tag("ｆｕｌｌｗｉｄｔｈ"), "fullwidth");
    }

    #[test]
    fn req_004_11_uses_javascripts_whitespace() {
        assert_eq!(normalize_tag("\u{feff}Holiday"), "holiday");
        assert_eq!(clean_tag("a\u{85}b"), "a\u{85}b");
    }

    #[test]
    fn category_roots_match_in_any_letter_case() {
        assert_eq!(category_of_root("Persons"), Some(TagCategory::People));
        assert_eq!(category_of_root(" LOCATIONS "), Some(TagCategory::Places));
        assert_eq!(category_of_root("Travel"), None);
    }
}
