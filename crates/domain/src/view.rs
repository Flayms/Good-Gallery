//! The gallery's view choices (spec 006, 016, 017) and what they show, stored as the Electron version stores them in
//! `localStorage`: `gallery.view`, `gallery.groupByDay`, `gallery.tagIndicators`.

use crate::layout::TileView;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GalleryView {
    #[default]
    Masonry,
    Justified,
    Grid,
    List,
}

impl GalleryView {
    pub const ALL: [Self; 4] = [Self::Masonry, Self::Justified, Self::Grid, Self::List];

    pub fn key(self) -> &'static str {
        match self {
            Self::Masonry => "masonry",
            Self::Justified => "justified",
            Self::Grid => "grid",
            Self::List => "list",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Masonry => "Masonry",
            Self::Justified => "Justified",
            Self::Grid => "Grid",
            Self::List => "List",
        }
    }

    /// The stored view; Masonry for anything else (REQ-006-01).
    pub fn parse(stored: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|view| Some(view.key()) == stored)
            .unwrap_or_default()
    }

    /// The tile layout of the view, `None` for the List view.
    pub fn tiles(self) -> Option<TileView> {
        match self {
            Self::Masonry => Some(TileView::Masonry),
            Self::Justified => Some(TileView::Justified),
            Self::Grid => Some(TileView::Grid),
            Self::List => None,
        }
    }
}

/// Whether the stored choice groups the gallery: it is grouped until the user turns it off (REQ-016-03).
pub fn is_grouped_by_day(stored: Option<&str>) -> bool {
    stored != Some("off")
}

pub fn group_by_day_value(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// Whether the gallery shows its day groups: grouping on and sorted by a date (REQ-016-03, REQ-016-04).
pub fn groups_shown(group_by_day: bool, date_sort: bool) -> bool {
    group_by_day && date_sort
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagIndicatorKind {
    Rating,
    Places,
    People,
    Tags,
}

impl TagIndicatorKind {
    pub const ALL: [Self; 4] = [Self::Rating, Self::Places, Self::People, Self::Tags];

    pub fn key(self) -> &'static str {
        match self {
            Self::Rating => "rating",
            Self::Places => "places",
            Self::People => "people",
            Self::Tags => "tags",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rating => "Rating",
            Self::Places => "Places",
            Self::People => "People",
            Self::Tags => "Tags",
        }
    }
}

/// Which tag indicators tiles show (REQ-017-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagIndicatorChoice {
    pub shown: bool,
    /// The picked indicators, in their fixed order; kept while `shown` is off.
    pub picked: Vec<TagIndicatorKind>,
}

impl Default for TagIndicatorChoice {
    /// Off, with every indicator picked.
    fn default() -> Self {
        Self {
            shown: false,
            picked: TagIndicatorKind::ALL.to_vec(),
        }
    }
}

impl TagIndicatorChoice {
    /// The stored choice: an object with `shown` and `picked`, unknown picks dropped; the default otherwise.
    pub fn parse(stored: Option<&Value>) -> Self {
        let Some(object) = stored.and_then(Value::as_object) else {
            return Self::default();
        };
        let (Some(shown), Some(picked)) = (object.get("shown"), object.get("picked")) else {
            return Self::default();
        };
        let picked: Vec<&str> = picked
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        Self {
            shown: shown == &Value::Bool(true),
            picked: TagIndicatorKind::ALL
                .into_iter()
                .filter(|kind| picked.contains(&kind.key()))
                .collect(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({ "shown": self.shown, "picked": self.picked.iter().map(|kind| kind.key()).collect::<Vec<_>>() })
    }

    /// Picks or unpicks an indicator, keeping the fixed order.
    pub fn toggle(&mut self, kind: TagIndicatorKind) {
        let picked = !self.picked.contains(&kind);
        self.picked = TagIndicatorKind::ALL
            .into_iter()
            .filter(|&k| if k == kind { picked } else { self.picked.contains(&k) })
            .collect();
    }
}

/// An indicator on a tile, with its name for tooltips and screen readers (REQ-017-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagIndicator {
    pub kind: TagIndicatorKind,
    pub label: String,
    /// Filled stars of a rating.
    pub stars: u8,
}

/// The indicators a tile shows, in a fixed order, leaving out what the media item lacks (REQ-017-01).
/// `tagged` is (places, people, other tags).
pub fn indicators_of(
    rating: Option<i64>,
    tagged: (bool, bool, bool),
    choice: &TagIndicatorChoice,
) -> Vec<TagIndicator> {
    if !choice.shown {
        return Vec::new();
    }
    let rating = rating.unwrap_or(0).clamp(0, 5);
    let (places, people, other) = tagged;
    TagIndicatorKind::ALL
        .into_iter()
        .filter(|kind| choice.picked.contains(kind))
        .filter(|kind| match kind {
            TagIndicatorKind::Rating => rating > 0,
            TagIndicatorKind::Places => places,
            TagIndicatorKind::People => people,
            TagIndicatorKind::Tags => other,
        })
        .map(|kind| match kind {
            TagIndicatorKind::Rating => TagIndicator {
                kind,
                label: format!("Rated {rating} out of 5"),
                stars: rating as u8,
            },
            _ => TagIndicator {
                kind,
                label: format!("Has {}", kind.label().to_lowercase()),
                stars: 0,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use TagIndicatorKind::*;

    fn choice(shown: bool, picked: &[TagIndicatorKind]) -> TagIndicatorChoice {
        TagIndicatorChoice {
            shown,
            picked: picked.to_vec(),
        }
    }

    fn kinds(indicators: &[TagIndicator]) -> Vec<TagIndicatorKind> {
        indicators.iter().map(|i| i.kind).collect()
    }

    const EVERYTHING: (Option<i64>, (bool, bool, bool)) = (Some(3), (true, true, true));

    #[test]
    fn req_017_01_req_017_02_shows_the_picked_indicators_in_a_fixed_order_with_stars_and_names() {
        let shown = indicators_of(
            EVERYTHING.0,
            EVERYTHING.1,
            &choice(true, &[Tags, People, Places, Rating]),
        );
        let expected = [
            (Rating, "Rated 3 out of 5", 3),
            (Places, "Has places", 0),
            (People, "Has people", 0),
            (Tags, "Has tags", 0),
        ];
        assert_eq!(
            shown,
            expected.map(|(kind, label, stars)| TagIndicator {
                kind,
                label: label.into(),
                stars
            })
        );
        assert_eq!(
            kinds(&indicators_of(EVERYTHING.0, EVERYTHING.1, &choice(true, &[People]))),
            [People]
        );
    }

    #[test]
    fn req_017_01_leaves_out_what_the_media_item_lacks() {
        let all = choice(true, &TagIndicatorKind::ALL);
        assert_eq!(kinds(&indicators_of(None, (false, true, false), &all)), [People]);
        assert!(indicators_of(Some(0), (false, true, false), &choice(true, &[Rating])).is_empty());
    }

    #[test]
    fn req_017_03_shows_nothing_while_tag_indicators_are_off() {
        assert!(indicators_of(EVERYTHING.0, EVERYTHING.1, &choice(false, &TagIndicatorKind::ALL)).is_empty());
    }

    #[test]
    fn req_017_03_is_off_with_all_indicators_picked_until_the_user_changes_it() {
        assert_eq!(TagIndicatorChoice::parse(None), choice(false, &TagIndicatorKind::ALL));
        assert_eq!(
            TagIndicatorChoice::parse(Some(&json!("not an object"))),
            TagIndicatorChoice::default()
        );
    }

    #[test]
    fn req_017_03_round_trips_keeping_the_picks_while_off_and_dropping_unknown_ones() {
        let stored = choice(false, &[Rating, People]);
        assert_eq!(TagIndicatorChoice::parse(Some(&stored.to_value())), stored);
        let gps = json!({"shown": true, "picked": ["rating", "gps"]});
        assert_eq!(TagIndicatorChoice::parse(Some(&gps)), choice(true, &[Rating]));
        let none = json!({"shown": true, "picked": []});
        assert_eq!(TagIndicatorChoice::parse(Some(&none)), choice(true, &[]));
    }

    #[test]
    fn req_017_03_toggles_picks_in_their_fixed_order() {
        let mut picks = choice(true, &[People]);
        picks.toggle(Rating);
        assert_eq!(picks.picked, [Rating, People]);
        picks.toggle(People);
        assert_eq!(picks.picked, [Rating]);
    }

    #[test]
    fn req_006_01_remembers_the_view_default_masonry() {
        assert_eq!(GalleryView::parse(None), GalleryView::Masonry);
        assert_eq!(GalleryView::parse(Some("list")), GalleryView::List);
        assert_eq!(GalleryView::parse(Some("carousel")), GalleryView::Masonry);
        assert!(
            GalleryView::ALL
                .iter()
                .all(|view| GalleryView::parse(Some(view.key())) == *view)
        );
    }

    #[test]
    fn req_016_03_is_grouped_until_the_user_turns_it_off() {
        assert!(is_grouped_by_day(None));
        assert!(is_grouped_by_day(Some("on")));
        assert!(!is_grouped_by_day(Some("off")));
        assert!(!is_grouped_by_day(Some(group_by_day_value(false))));
    }

    #[test]
    fn req_016_04_shows_groups_only_for_date_sorts_while_grouping_is_on() {
        assert!(groups_shown(true, true));
        assert!(!groups_shown(false, true));
        assert!(!groups_shown(true, false));
    }
}
