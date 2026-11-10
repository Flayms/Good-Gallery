//! The data of the date calendar (spec 041) and the world map (spec 044), loaded for the search shown.

use super::*;

/// What the date calendar asks for: the search apart from its date range, and the days it shows (REQ-041-02).
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarKey {
    search: GallerySearch,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
    /// The month view wants each day's first thumbnail; the year view only counts.
    thumbnails: bool,
}

/// A day of the date calendar: its photos and videos, and its first item with its thumbnail (REQ-041-02).
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarDay {
    pub photos: u32,
    pub videos: u32,
    pub first: Option<(i64, ThumbKey)>,
}

/// The days the calendar asked for that have items, and their date groups for the year view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CalendarData {
    pub days: std::collections::HashMap<chrono::NaiveDate, CalendarDay>,
    pub groups: Vec<domain::date_groups::DateGroup>,
}

/// Where the date calendar is: the month it shows, whether it shows that month's year, and the first day of a range
/// being picked (REQ-041-01, REQ-041-03). Kept while the menu is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarView {
    pub month: chrono::NaiveDate,
    pub year_view: bool,
    pub anchor: Option<chrono::NaiveDate>,
}

pub(super) fn load_calendar(
    conn: &Connection,
    key: &CalendarKey,
    content: &ContentSlot,
) -> Result<CalendarData, String> {
    let search = GallerySearch {
        from: Some(key.from),
        to: Some(key.to),
        sort_by: domain::search::SortBy::Taken,
        descending: false,
        ..key.search.clone()
    };
    let search = resolve(conn, &search, content)?;
    let layout = db::gallery::layout(conn, &search).map_err(|e| e.to_string())?;
    let mut days = std::collections::HashMap::new();
    for group in &layout.groups {
        let first = match layout.ids.get(group.start) {
            Some(&id) if key.thumbnails => db::thumbs::thumb_source(conn, id)
                .map_err(|e| e.to_string())?
                .map(|source| (id, source.key)),
            _ => None,
        };
        days.insert(
            group.day,
            CalendarDay {
                photos: group.photos,
                videos: group.videos,
                first,
            },
        );
    }
    Ok(CalendarData {
        days,
        groups: layout.groups,
    })
}

/// The map's points: id, longitude, latitude; newest first.
pub(super) type MapPoints = Vec<(i64, f64, f64)>;
/// Clusters of the map's points at a zoom level.
pub(super) type MapClusters = (Arc<MapPoints>, f64, Arc<Vec<domain::map::Cluster>>);

impl Gallery {
    /// Where the date calendar is, opening at the month of the range's start, else this month (REQ-041-01).
    pub fn calendar_view(&mut self) -> &mut CalendarView {
        let month = domain::calendar::month_of(self.search.from.unwrap_or_else(domain::time_zone::today));
        self.calendar.get_or_insert(CalendarView {
            month,
            year_view: false,
            anchor: None,
        })
    }

    /// The items on the days `from` to `to` among those the search shows apart from its date range, with each
    /// day's first thumbnail if `thumbnails`; the last answer while a new one loads (REQ-041-02, REQ-041-03).
    pub fn calendar_days(
        &mut self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
        thumbnails: bool,
    ) -> Option<&CalendarData> {
        let key = CalendarKey {
            search: GallerySearch {
                from: None,
                to: None,
                ..self.search.clone()
            },
            from,
            to,
            thumbnails,
        };
        if self.calendar_asked.as_ref() != Some(&key) {
            self.calendar_asked = Some(key.clone());
            self.loader.send(Request::Calendar(key));
        }
        self.calendar_data.as_ref().map(|(_, data)| data)
    }

    /// Whether the calendar's data is for what it asked last.
    pub fn calendar_is_current(&self) -> bool {
        matches!((&self.calendar_asked, &self.calendar_data), (Some(asked), Some((key, _))) if asked == key)
    }

    /// Asks for the calendar's thumbnails before the tiles' in the next frame, or now when no tiles asked for theirs
    /// in this one (none match, or another page) (REQ-041-02).
    pub fn want_calendar_thumbnails(&mut self, wanted: Vec<(i64, ThumbKey)>, pixels_per_point: f32) {
        self.calendar_wanted = wanted;
        if !self.textures_asked {
            self.request_textures(std::iter::empty(), pixels_per_point);
        }
    }

    /// The located items of the gallery's search, apart from photos chosen on the map, newest first; the last answer
    /// while a new one loads (REQ-044-01).
    pub fn map_points(&mut self) -> Option<Arc<Vec<(i64, f64, f64)>>> {
        let search = GallerySearch {
            media: Vec::new(),
            ..self.search.clone()
        };
        if self.map_asked.as_ref() != Some(&search) {
            self.map_asked = Some(search.clone());
            self.loader.send(Request::Located(search));
        }
        self.map_points.clone()
    }

    /// The clusters of the map's `points` at zoom `level` (REQ-044-01, REQ-044-02), grouped again only when either
    /// changes.
    pub fn map_clusters(
        &mut self,
        points: &Arc<MapPoints>,
        level: f64,
        spacing: f64,
    ) -> Arc<Vec<domain::map::Cluster>> {
        match &self.map_clusters {
            Some((of, at, clusters)) if Arc::ptr_eq(of, points) && *at == level => clusters.clone(),
            _ => {
                let clusters = Arc::new(domain::map::clusters(points, level, spacing));
                self.map_clusters = Some((points.clone(), level, clusters.clone()));
                clusters
            }
        }
    }

    /// The thumbnail key of photo `id`, asked for when first wanted; `None` while unknown or for one without.
    pub fn map_key(&mut self, id: i64) -> Option<ThumbKey> {
        if let Some(key) = self.map_keys.get(&id) {
            return key.clone();
        }
        if self.map_keys_asked.insert(id) {
            self.loader.send(Request::ThumbKeys(vec![id]));
        }
        None
    }
}
