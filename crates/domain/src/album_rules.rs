//! Smart album rules (spec 039): a flat list of conditions matching all or any, from and to a search, relative dates,
//! and how rules are stored.

use crate::media::MediaKind;
use crate::search::{GallerySearch, SortBy};
use chrono::{Datelike, Months, NaiveDate};
use serde_json::{Value, json};

/// A unit of a relative date (REQ-039-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Days,
    Weeks,
    Months,
    Years,
}

impl Unit {
    pub const ALL: [Self; 4] = [Self::Days, Self::Weeks, Self::Months, Self::Years];

    pub fn key(self) -> &'static str {
        match self {
            Self::Days => "days",
            Self::Weeks => "weeks",
            Self::Months => "months",
            Self::Years => "years",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|unit| unit.key() == key)
    }
}

/// Which date of a media item a date condition looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateField {
    /// Date taken, else date modified, as the gallery sorts (REQ-007-08).
    Taken,
    Modified,
}

/// The days a date condition covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Days {
    /// Both inclusive; either may be open.
    Between(Option<NaiveDate>, Option<NaiveDate>),
    /// The last `n` units up to and including today (REQ-039-04).
    Last(u32, Unit),
}

impl Days {
    /// The first and last day for `today`; `None` for an open end.
    pub fn range(self, today: NaiveDate) -> (Option<NaiveDate>, Option<NaiveDate>) {
        match self {
            Self::Between(from, to) => (from, to),
            Self::Last(n, unit) => {
                let start = match unit {
                    Unit::Days => today.checked_sub_days(chrono::Days::new(u64::from(n.saturating_sub(1)))),
                    Unit::Weeks => today.checked_sub_days(chrono::Days::new(u64::from(n) * 7 - 1)),
                    Unit::Months => today.checked_sub_months(Months::new(n)).and_then(|day| day.succ_opt()),
                    Unit::Years => today
                        .with_year(today.year() - n as i32)
                        .or_else(|| today.pred_opt()?.with_year(today.year() - n as i32))
                        .and_then(|day| day.succ_opt()),
                };
                (start, Some(today))
            }
        }
    }
}

/// How a photo is shaped (REQ-039-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
    Square,
}

impl Orientation {
    pub const ALL: [Self; 3] = [Self::Portrait, Self::Landscape, Self::Square];

    pub fn key(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
            Self::Square => "square",
        }
    }
}

/// A number a condition compares with: at least or at most.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bound {
    pub at_least: bool,
    pub value: f64,
}

// A search is compared whole (an open smart album stays marked while it is unchanged); a bound is never NaN once
// complete, so equality is total where it matters.
impl Eq for Bound {}

/// One condition of a smart album (REQ-039-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// Carries the tag or one of its descendants, or doesn't (`is: false`).
    Tag {
        name: String,
        is: bool,
    },
    HasPeople,
    HasPlaces,
    NoTags,
    NameContains(String),
    Kind(MediaKind),
    /// One of these ratings, 0 meaning unrated.
    RatingIn(Vec<u8>),
    RatingAtLeast(u8),
    Library(i64),
    /// A folder of a library, with its subfolders.
    Folder {
        library: i64,
        folder: String,
    },
    Date {
        field: DateField,
        days: Days,
    },
    /// Bytes.
    Size(Bound),
    /// Seconds.
    Duration(Bound),
    Megapixels(Bound),
    Orientation(Orientation),
    /// Content search words (REQ-036-05); at most one per album.
    Shows(String),
    /// The items content words matched, put in place of [`Condition::Shows`] before a query; never stored.
    Matches(Vec<i64>),
}

impl Condition {
    /// It has every value it needs (REQ-039-02).
    pub fn is_complete(&self) -> bool {
        match self {
            Self::Tag { name, .. } | Self::NameContains(name) | Self::Shows(name) => !name.trim().is_empty(),
            Self::RatingIn(ratings) => !ratings.is_empty(),
            Self::Folder { folder, .. } => !folder.trim_matches('/').is_empty(),
            Self::Date {
                days: Days::Between(from, to),
                ..
            } => from.is_some() || to.is_some(),
            Self::Date {
                days: Days::Last(n, _), ..
            } => *n > 0,
            Self::Size(bound) | Self::Duration(bound) | Self::Megapixels(bound) => {
                bound.value.is_finite() && bound.value >= 0.0
            }
            _ => true,
        }
    }

    /// It needs today's date, so its matches change with the day (REQ-039-04).
    pub fn is_relative(&self) -> bool {
        matches!(
            self,
            Self::Date {
                days: Days::Last(..),
                ..
            }
        )
    }
}

/// A smart album's rules (REQ-039-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// The album's name, shown as the filters bar's chip (REQ-039-05); not stored with the rules.
    pub name: String,
    /// Match any condition instead of all.
    pub any: bool,
    pub conditions: Vec<Condition>,
}

impl Rules {
    /// The filters of `search` as conditions matching all (REQ-039-01, REQ-039-06).
    pub fn from_search(search: &GallerySearch) -> Self {
        let mut conditions = Vec::new();
        match (search.library, search.folder_filter()) {
            (Some(library), Some(folder)) => conditions.push(Condition::Folder {
                library,
                folder: folder.to_owned(),
            }),
            (Some(library), None) => conditions.push(Condition::Library(library)),
            (None, _) => {}
        }
        if let Some(kind) = search.kind {
            conditions.push(Condition::Kind(kind));
        }
        if let Some(name) = search.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            conditions.push(Condition::NameContains(name.to_owned()));
        }
        conditions.extend(search.tags.iter().map(|name| Condition::Tag {
            name: name.clone(),
            is: true,
        }));
        conditions.extend(search.exclude.iter().map(|name| Condition::Tag {
            name: name.clone(),
            is: false,
        }));
        if search.from.is_some() || search.to.is_some() {
            conditions.push(Condition::Date {
                field: DateField::Taken,
                days: Days::Between(search.from, search.to),
            });
        }
        if !search.ratings.is_empty() {
            conditions.push(Condition::RatingIn(search.ratings.clone()));
        }
        if let Some(words) = &search.content {
            conditions.push(Condition::Shows(words.clone()));
        }
        Self {
            name: String::new(),
            any: false,
            conditions,
        }
    }

    /// The search these rules are, if the filters bar can show them: matching all, with conditions it has
    /// (REQ-039-05). Sort order isn't part of the rules.
    pub fn as_search(&self) -> Option<GallerySearch> {
        if self.any {
            return None;
        }
        let mut search = GallerySearch::default();
        let mut dated = false;
        for condition in &self.conditions {
            match condition {
                Condition::Library(library) if search.library.is_none() => search.library = Some(*library),
                Condition::Folder { library, folder } if search.library.is_none() => {
                    search.library = Some(*library);
                    search.folder = Some(folder.clone());
                }
                Condition::Kind(kind) if search.kind.is_none() => search.kind = Some(*kind),
                Condition::NameContains(name) if search.name.is_none() => search.name = Some(name.clone()),
                Condition::Tag { name, is: true } => search.tags.push(name.clone()),
                Condition::Tag { name, is: false } => search.exclude.push(name.clone()),
                Condition::Date {
                    field: DateField::Taken,
                    days: Days::Between(from, to),
                } if !dated => {
                    (search.from, search.to, dated) = (*from, *to, true);
                }
                Condition::RatingIn(ratings) if search.ratings.is_empty() => {
                    let mut ratings = ratings.clone();
                    ratings.sort_unstable();
                    ratings.dedup();
                    search.ratings = ratings;
                }
                Condition::Shows(words) if search.content.is_none() => search.content = Some(words.clone()),
                _ => return None,
            }
        }
        Some(search)
    }

    pub fn is_complete(&self) -> bool {
        self.conditions.iter().all(Condition::is_complete)
    }

    pub fn is_relative(&self) -> bool {
        self.conditions.iter().any(Condition::is_relative)
    }

    /// The content words of its _Shows_ condition, if it has one.
    pub fn content(&self) -> Option<&str> {
        self.conditions.iter().find_map(|condition| match condition {
            Condition::Shows(words) => Some(words.as_str()),
            _ => None,
        })
    }

    /// These rules with their content words replaced by the items they match (REQ-036-05).
    pub fn with_content_matches(&self, matches: &[i64]) -> Self {
        let conditions = self
            .conditions
            .iter()
            .map(|condition| match condition {
                Condition::Shows(_) => Condition::Matches(matches.to_vec()),
                other => other.clone(),
            })
            .collect();
        Self {
            conditions,
            ..self.clone()
        }
    }
}

/// How rules are stored with their sort order (REQ-039-06): version 2 of a smart album's JSON.
pub fn rules_to_value(rules: &Rules, sort_by: SortBy, descending: bool) -> Value {
    let day = |day: Option<NaiveDate>| day.map_or(Value::Null, |day| json!(day.to_string()));
    let bound = |kind: &str, bound: &Bound| json!({"type": kind, "at_least": bound.at_least, "value": bound.value});
    let conditions: Vec<Value> = rules
        .conditions
        .iter()
        .filter_map(|condition| {
            Some(match condition {
                Condition::Tag { name, is } => json!({"type": "tag", "name": name, "is": is}),
                Condition::HasPeople => json!({"type": "has_people"}),
                Condition::HasPlaces => json!({"type": "has_places"}),
                Condition::NoTags => json!({"type": "no_tags"}),
                Condition::NameContains(name) => json!({"type": "name", "contains": name}),
                Condition::Kind(kind) => json!({"type": "kind", "is": kind.as_str()}),
                Condition::RatingIn(ratings) => json!({"type": "rating_in", "ratings": ratings}),
                Condition::RatingAtLeast(stars) => json!({"type": "rating_at_least", "stars": stars}),
                Condition::Library(library) => json!({"type": "library", "id": library}),
                Condition::Folder { library, folder } => {
                    json!({"type": "folder", "library": library, "folder": folder})
                }
                Condition::Date { field, days } => {
                    let field = match field {
                        DateField::Taken => "taken",
                        DateField::Modified => "modified",
                    };
                    match days {
                        Days::Between(from, to) => {
                            json!({"type": "date", "field": field, "from": day(*from), "to": day(*to)})
                        }
                        Days::Last(n, unit) => json!({"type": "date", "field": field, "last": n, "unit": unit.key()}),
                    }
                }
                Condition::Size(b) => bound("size", b),
                Condition::Duration(b) => bound("duration", b),
                Condition::Megapixels(b) => bound("megapixels", b),
                Condition::Orientation(orientation) => json!({"type": "orientation", "is": orientation.key()}),
                Condition::Shows(words) => json!({"type": "shows", "words": words}),
                Condition::Matches(_) => return None,
            })
        })
        .collect();
    json!({
        "version": 2,
        "match": if rules.any { "any" } else { "all" },
        "conditions": conditions,
        "sort_by": sort_by.key(),
        "descending": descending,
    })
}

/// Stored rules, if `value` is version 2; conditions that can't be read are left out.
pub fn rules_from_value(value: &Value) -> Option<(Rules, SortBy, bool)> {
    if value.get("version").and_then(Value::as_i64) != Some(2) {
        return None;
    }
    let conditions = value
        .get("conditions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(condition_from_value)
        .collect();
    let rules = Rules {
        name: String::new(),
        any: value.get("match").and_then(Value::as_str) == Some("any"),
        conditions,
    };
    let default = GallerySearch::default();
    let sort_by = value
        .get("sort_by")
        .and_then(Value::as_str)
        .and_then(SortBy::from_key)
        .unwrap_or(default.sort_by);
    let descending = value
        .get("descending")
        .and_then(Value::as_bool)
        .unwrap_or(default.descending);
    Some((rules, sort_by, descending))
}

fn condition_from_value(value: &Value) -> Option<Condition> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let day = |key: &str| text(key).and_then(|day| day.parse::<NaiveDate>().ok());
    let bound = || {
        Some(Bound {
            at_least: value.get("at_least")?.as_bool()?,
            value: value.get("value")?.as_f64()?,
        })
    };
    Some(match value.get("type")?.as_str()? {
        "tag" => Condition::Tag {
            name: text("name")?,
            is: value.get("is")?.as_bool()?,
        },
        "has_people" => Condition::HasPeople,
        "has_places" => Condition::HasPlaces,
        "no_tags" => Condition::NoTags,
        "name" => Condition::NameContains(text("contains")?),
        "kind" => Condition::Kind(MediaKind::parse(&text("is")?)?),
        "rating_in" => Condition::RatingIn(
            value
                .get("ratings")?
                .as_array()?
                .iter()
                .filter_map(Value::as_u64)
                .filter(|&rating| rating <= 5)
                .map(|rating| rating as u8)
                .collect(),
        ),
        "rating_at_least" => Condition::RatingAtLeast(u8::try_from(value.get("stars")?.as_u64()?.min(5)).ok()?),
        "library" => Condition::Library(value.get("id")?.as_i64()?),
        "folder" => Condition::Folder {
            library: value.get("library")?.as_i64()?,
            folder: text("folder")?,
        },
        "date" => {
            let field = match text("field")?.as_str() {
                "taken" => DateField::Taken,
                "modified" => DateField::Modified,
                _ => return None,
            };
            let days = match value.get("last").and_then(Value::as_u64) {
                Some(n) => Days::Last(u32::try_from(n).ok()?, Unit::from_key(&text("unit")?)?),
                None => Days::Between(day("from"), day("to")),
            };
            Condition::Date { field, days }
        }
        "size" => Condition::Size(bound()?),
        "duration" => Condition::Duration(bound()?),
        "megapixels" => Condition::Megapixels(bound()?),
        "orientation" => {
            let key = text("is")?;
            Condition::Orientation(Orientation::ALL.into_iter().find(|o| o.key() == key)?)
        }
        "shows" => Condition::Shows(text("words")?),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    fn rules(any: bool, conditions: Vec<Condition>) -> Rules {
        Rules {
            name: String::new(),
            any,
            conditions,
        }
    }

    /// REQ-039-04: the last N days, weeks, months and years end today, and move with it.
    #[test]
    fn req_039_04_relative_dates_end_today() {
        let today = day("2026-03-31");
        let last = |n, unit| Days::Last(n, unit).range(today);
        assert_eq!(last(1, Unit::Days), (Some(today), Some(today)));
        assert_eq!(last(30, Unit::Days), (Some(day("2026-03-02")), Some(today)));
        assert_eq!(last(2, Unit::Weeks), (Some(day("2026-03-18")), Some(today)));
        assert_eq!(last(1, Unit::Months), (Some(day("2026-03-01")), Some(today)));
        assert_eq!(last(1, Unit::Years), (Some(day("2025-04-01")), Some(today)));
        assert_eq!(
            Days::Last(1, Unit::Years).range(day("2028-02-29")).0,
            Some(day("2027-03-01"))
        );
        let between = Days::Between(Some(day("2020-01-01")), None);
        assert_eq!(between.range(today), (Some(day("2020-01-01")), None));
    }

    /// REQ-039-01, REQ-039-06: a search's filters become conditions matching all, and back.
    #[test]
    fn req_039_01_req_039_06_a_search_becomes_rules_and_back() {
        let search = GallerySearch {
            library: Some(3),
            folder: Some("2023/Rome".into()),
            kind: Some(MediaKind::Video),
            name: Some("IMG".into()),
            tags: vec!["Rome".into()],
            exclude: vec!["Blurry".into()],
            from: Some(day("2023-05-01")),
            to: Some(day("2023-05-14")),
            ratings: vec![0, 4],
            content: Some("a dog".into()),
            ..GallerySearch::default()
        };
        let rules = Rules::from_search(&search);
        assert!(!rules.any);
        assert_eq!(
            rules.conditions,
            [
                Condition::Folder {
                    library: 3,
                    folder: "2023/Rome".into()
                },
                Condition::Kind(MediaKind::Video),
                Condition::NameContains("IMG".into()),
                Condition::Tag {
                    name: "Rome".into(),
                    is: true
                },
                Condition::Tag {
                    name: "Blurry".into(),
                    is: false
                },
                Condition::Date {
                    field: DateField::Taken,
                    days: Days::Between(Some(day("2023-05-01")), Some(day("2023-05-14")))
                },
                Condition::RatingIn(vec![0, 4]),
                Condition::Shows("a dog".into()),
            ]
        );
        assert_eq!(rules.as_search(), Some(search));
        let library = Rules::from_search(&GallerySearch {
            library: Some(3),
            ..GallerySearch::default()
        });
        assert_eq!(library.conditions, [Condition::Library(3)]);
        assert_eq!(Rules::from_search(&GallerySearch::default()).conditions, []);
    }

    /// REQ-039-05: rules the filters bar can't show aren't a search.
    #[test]
    fn req_039_05_rules_the_filters_bar_cant_show() {
        let tag = || Condition::Tag {
            name: "Beach".into(),
            is: true,
        };
        assert!(rules(false, vec![tag()]).as_search().is_some());
        assert!(rules(true, vec![tag()]).as_search().is_none(), "any");
        for condition in [
            Condition::Date {
                field: DateField::Taken,
                days: Days::Last(30, Unit::Days),
            },
            Condition::Date {
                field: DateField::Modified,
                days: Days::Between(Some(day("2020-01-01")), None),
            },
            Condition::Size(Bound {
                at_least: true,
                value: 1e6,
            }),
            Condition::Duration(Bound {
                at_least: true,
                value: 60.0,
            }),
            Condition::Megapixels(Bound {
                at_least: false,
                value: 2.0,
            }),
            Condition::Orientation(Orientation::Portrait),
            Condition::HasPeople,
            Condition::HasPlaces,
            Condition::NoTags,
            Condition::RatingAtLeast(3),
        ] {
            assert!(
                rules(false, vec![tag(), condition.clone()]).as_search().is_none(),
                "{condition:?}"
            );
        }
        // Two of what the bar has once.
        let kinds = vec![Condition::Kind(MediaKind::Image), Condition::Kind(MediaKind::Video)];
        assert!(rules(false, kinds).as_search().is_none());
    }

    /// REQ-039-02: a condition without its value is incomplete; relative dates are noticed.
    #[test]
    fn req_039_02_incomplete_conditions() {
        assert!(
            !Condition::Tag {
                name: " ".into(),
                is: true
            }
            .is_complete()
        );
        assert!(!Condition::RatingIn(vec![]).is_complete());
        assert!(
            !Condition::Date {
                field: DateField::Taken,
                days: Days::Between(None, None)
            }
            .is_complete()
        );
        assert!(
            !Condition::Date {
                field: DateField::Taken,
                days: Days::Last(0, Unit::Days)
            }
            .is_complete()
        );
        assert!(
            !Condition::Size(Bound {
                at_least: true,
                value: f64::NAN
            })
            .is_complete()
        );
        assert!(Condition::HasPeople.is_complete());
        let relative = rules(
            false,
            vec![Condition::Date {
                field: DateField::Taken,
                days: Days::Last(3, Unit::Weeks),
            }],
        );
        assert!(relative.is_relative());
        assert!(!rules(false, vec![Condition::NoTags]).is_relative());
    }

    /// REQ-039-06: rules are stored as version 2 with their sort order, and read back; what can't be read is left
    /// out, and an old search isn't rules.
    #[test]
    fn req_039_06_stores_and_reads_rules() {
        let all = rules(
            true,
            vec![
                Condition::Tag {
                    name: "Beach".into(),
                    is: false,
                },
                Condition::HasPeople,
                Condition::HasPlaces,
                Condition::NoTags,
                Condition::NameContains("IMG".into()),
                Condition::Kind(MediaKind::Image),
                Condition::RatingIn(vec![0, 5]),
                Condition::RatingAtLeast(3),
                Condition::Library(2),
                Condition::Folder {
                    library: 2,
                    folder: "a/b".into(),
                },
                Condition::Date {
                    field: DateField::Modified,
                    days: Days::Last(2, Unit::Months),
                },
                Condition::Date {
                    field: DateField::Taken,
                    days: Days::Between(None, Some(day("2020-01-01"))),
                },
                Condition::Size(Bound {
                    at_least: true,
                    value: 1_000_000.0,
                }),
                Condition::Duration(Bound {
                    at_least: false,
                    value: 60.0,
                }),
                Condition::Megapixels(Bound {
                    at_least: true,
                    value: 12.0,
                }),
                Condition::Orientation(Orientation::Square),
                Condition::Shows("a dog".into()),
            ],
        );
        let value = rules_to_value(&all, SortBy::Name, false);
        assert_eq!(value["version"], 2);
        assert_eq!(rules_from_value(&value), Some((all, SortBy::Name, false)));
        assert_eq!(rules_from_value(&json!({"tags": ["Rome"]})), None);
        let odd = json!({"version": 2, "match": "any", "conditions": [{"type": "colour"}, {"type": "no_tags"}]});
        let (read, sort_by, descending) = rules_from_value(&odd).unwrap();
        assert_eq!(read.conditions, [Condition::NoTags]);
        assert!(read.any);
        assert_eq!((sort_by, descending), (SortBy::Taken, true));
    }
}
