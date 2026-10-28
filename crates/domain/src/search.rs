//! The search the gallery shows (spec 007): filters and sort order.

use crate::media::MediaKind;
use crate::tags::normalize_tag;
use chrono::{Local, NaiveDate, TimeZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    Name,
    /// Date taken, else the date modified.
    #[default]
    Taken,
    Modified,
    Path,
    Size,
}

impl SortBy {
    pub const ALL: [Self; 5] = [Self::Taken, Self::Modified, Self::Name, Self::Path, Self::Size];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Taken => "Date taken",
            Self::Modified => "Date modified",
            Self::Path => "Full path",
            Self::Size => "Size",
        }
    }

    /// Date sorts group by day (REQ-016-04).
    pub fn is_date(self) -> bool {
        matches!(self, Self::Taken | Self::Modified)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagMode {
    Include,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GallerySearch {
    pub library: Option<i64>,
    /// `/`-separated folder below the library, including subfolders; ignored without a library (REQ-007-10).
    pub folder: Option<String>,
    pub kind: Option<MediaKind>,
    /// Case-insensitive substring of the file name.
    pub name: Option<String>,
    /// Tag display names: media items must carry all of them (REQ-007-01).
    pub tags: Vec<String>,
    /// Tag display names: media items must carry none of them (REQ-007-02).
    pub exclude: Vec<String>,
    /// Local calendar days, both inclusive (REQ-007-08).
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// Ratings to match, `0` meaning unrated; empty for any (REQ-007-09).
    pub ratings: Vec<u8>,
    pub sort_by: SortBy,
    pub descending: bool,
}

impl Default for GallerySearch {
    /// Everything, by date taken, newest first (REQ-007-12).
    fn default() -> Self {
        Self {
            library: None,
            folder: None,
            kind: None,
            name: None,
            tags: Vec::new(),
            exclude: Vec::new(),
            from: None,
            to: None,
            ratings: Vec::new(),
            sort_by: SortBy::Taken,
            descending: true,
        }
    }
}

/// Milliseconds of the local midnight starting `day` (the first existing local time if daylight saving skips it;
/// UTC midnight if the time zone has none at all).
fn start_of(day: NaiveDate) -> i64 {
    (0..24)
        .find_map(|hour| Local.from_local_datetime(&day.and_hms_opt(hour, 0, 0)?).earliest())
        .map_or_else(
            || day.and_time(chrono::NaiveTime::MIN).and_utc().timestamp_millis(),
            |time| time.timestamp_millis(),
        )
}

impl GallerySearch {
    /// Removes a tag from both lists, compared by normalized name.
    pub fn without_tag(&mut self, name: &str) {
        let norm = normalize_tag(name);
        self.tags.retain(|other| normalize_tag(other) != norm);
        self.exclude.retain(|other| normalize_tag(other) != norm);
    }

    /// Adds a tag, or moves it to the other list: a tag is never included and excluded at once (REQ-007-05).
    pub fn with_tag(&mut self, name: &str, mode: TagMode) {
        self.without_tag(name);
        match mode {
            TagMode::Include => self.tags.push(name.to_string()),
            TagMode::Exclude => self.exclude.push(name.to_string()),
        }
    }

    pub fn tag_mode(&self, name: &str) -> Option<TagMode> {
        let norm = normalize_tag(name);
        let has = |names: &[String]| names.iter().any(|other| normalize_tag(other) == norm);
        if has(&self.tags) {
            Some(TagMode::Include)
        } else if has(&self.exclude) {
            Some(TagMode::Exclude)
        } else {
            None
        }
    }

    /// Adds a rating to the rating filter, or removes it if it is there; kept in order, so the same selection is the
    /// same search (REQ-007-09, REQ-008-04).
    pub fn toggle_rating(&mut self, rating: u8) {
        match self.ratings.binary_search(&rating) {
            Ok(at) => {
                self.ratings.remove(at);
            }
            Err(at) => self.ratings.insert(at, rating),
        }
    }

    /// How many of the filters under _Filters_ are set: media kind, date range (once), rating (REQ-013-08).
    pub fn active_filter_count(&self) -> usize {
        [
            self.kind.is_some(),
            self.from.is_some() || self.to.is_some(),
            !self.ratings.is_empty(),
        ]
        .into_iter()
        .filter(|&set| set)
        .count()
    }

    /// Removes the filters under _Filters_, keeping tags, file name, library, folder and sorting.
    pub fn clear_filters(&mut self) {
        self.kind = None;
        self.from = None;
        self.to = None;
        self.ratings.clear();
    }

    /// The folder filter, only with a library (REQ-007-10); leading and trailing `/` dropped.
    pub fn folder_filter(&self) -> Option<&str> {
        self.library?;
        let folder = self.folder.as_deref()?.trim_matches('/');
        (!folder.is_empty()).then_some(folder)
    }

    /// The date range as milliseconds: from the start of `from` to the start of the day after `to` (REQ-007-08).
    pub fn date_range(&self) -> (Option<i64>, Option<i64>) {
        (
            self.from.map(start_of),
            self.to.map(|to| start_of(to.succ_opt().unwrap_or(to))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_007_05_req_008_06_adds_moves_and_removes_tags_case_insensitively() {
        let mut search = GallerySearch::default();
        search.with_tag("Berlin", TagMode::Include);
        assert_eq!(
            (search.tags.clone(), search.exclude.clone()),
            (vec!["Berlin".to_string()], vec![])
        );

        search.with_tag("berlin", TagMode::Exclude);
        assert_eq!(
            (search.tags.clone(), search.exclude.clone()),
            (vec![], vec!["berlin".to_string()])
        );
        assert_eq!(search.tag_mode("BERLIN"), Some(TagMode::Exclude));

        search.tags.push("Paris".into());
        search.without_tag("Berlin");
        assert_eq!(
            (search.tags.clone(), search.exclude.clone()),
            (vec!["Paris".to_string()], vec![])
        );
        assert_eq!(search.tag_mode("Rome"), None);
    }

    #[test]
    fn req_007_09_toggles_ratings_in_order() {
        let mut search = GallerySearch::default();
        for rating in [5, 0, 3] {
            search.toggle_rating(rating);
        }
        assert_eq!(search.ratings, [0, 3, 5]);
        search.toggle_rating(3);
        assert_eq!(search.ratings, [0, 5]);
    }

    #[test]
    fn req_007_08_turns_inclusive_local_days_into_a_half_open_range() {
        let search = GallerySearch {
            from: NaiveDate::from_ymd_opt(2024, 3, 30),
            to: NaiveDate::from_ymd_opt(2024, 3, 31),
            ..Default::default()
        };
        let local = |d| {
            Local
                .with_ymd_and_hms(2024, if d == 1 { 4 } else { 3 }, d, 0, 0, 0)
                .earliest()
                .unwrap()
        };
        assert_eq!(
            search.date_range(),
            (Some(local(30).timestamp_millis()), Some(local(1).timestamp_millis()))
        );
        assert_eq!(GallerySearch::default().date_range(), (None, None));
    }

    #[test]
    fn req_007_10_drops_the_folder_without_a_library() {
        let folder = |library, folder: &str| GallerySearch {
            library,
            folder: Some(folder.into()),
            ..Default::default()
        };
        assert_eq!(folder(None, "a").folder_filter(), None);
        assert_eq!(folder(Some(1), "a/b/").folder_filter(), Some("a/b"));
        assert_eq!(folder(Some(1), "/").folder_filter(), None);
    }

    #[test]
    fn req_013_08_counts_and_clears_only_the_filters_under_filters() {
        let mut search = GallerySearch::default();
        assert_eq!(search.active_filter_count(), 0);
        search.kind = Some(MediaKind::Video);
        assert_eq!(search.active_filter_count(), 1);
        search.from = NaiveDate::from_ymd_opt(2024, 1, 1);
        search.to = NaiveDate::from_ymd_opt(2024, 12, 31);
        search.ratings = vec![3, 5];
        assert_eq!(search.active_filter_count(), 3);

        search.tags = vec!["Beach".into()];
        search.name = Some("IMG".into());
        search.library = Some(1);
        search.folder = Some("a".into());
        search.sort_by = SortBy::Name;
        search.clear_filters();
        assert_eq!(search.active_filter_count(), 0);
        assert_eq!(
            (search.tags.len(), search.library, search.sort_by),
            (1, Some(1), SortBy::Name)
        );
    }
}
