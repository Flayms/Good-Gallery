//! The years beside the gallery's scrollbar (REQ-006-16): which years there are in a result, where they go on the bar,
//! and which of them fit.

use crate::date_groups::DateGroup;
use chrono::Datelike;

/// Where a year starts in a date-sorted result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YearMark {
    pub year: i32,
    /// Gallery index of the year's first item.
    pub first_item: usize,
}

/// The first day group of each run of a year, in gallery order.
pub fn year_marks(groups: &[DateGroup]) -> Vec<YearMark> {
    let mut marks: Vec<YearMark> = Vec::new();
    for group in groups {
        let year = group.day.year();
        if marks.last().is_none_or(|mark| mark.year != year) {
            marks.push(YearMark {
                year,
                first_item: group.start,
            });
        }
    }
    marks
}

/// The geometry of a scrollbar: its track on screen, in points, and what it scrolls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollTrack {
    pub top: f64,
    pub bottom: f64,
    /// Height of the content and of the part of it in view.
    pub content: f64,
    pub view: f64,
    pub min_handle: f64,
}

impl ScrollTrack {
    /// The furthest the content scrolls.
    pub fn max_offset(&self) -> f64 {
        (self.content - self.view).max(0.0)
    }

    /// The middle of the thumb at scroll `offset`. A click on the track centers the thumb on the pointer, so a click
    /// at this height scrolls to `offset`.
    pub fn thumb_center(&self, offset: f64) -> f64 {
        let track = self.bottom - self.top;
        let handle = (self.view / self.content * track).max(self.min_handle);
        let travel = track - handle;
        let at = (offset / self.max_offset()).clamp(0.0, 1.0);
        self.top + at * travel + handle / 2.0
    }

    /// The scroll offset with the thumb's middle at `y`: the inverse of [`Self::thumb_center`], clamped to the
    /// thumb's travel (REQ-006-19).
    pub fn offset_at(&self, y: f64) -> f64 {
        let track = self.bottom - self.top;
        let handle = (self.view / self.content * track).max(self.min_handle);
        let travel = track - handle;
        if travel <= 0.0 {
            return 0.0;
        }
        ((y - self.top - handle / 2.0) / travel).clamp(0.0, 1.0) * self.max_offset()
    }
}

/// Indices of the heights to keep, in order, so that no two are closer than `min_gap`: from the first on, a height too
/// close to the last one kept is dropped.
pub fn spread(heights: &[f64], min_gap: f64) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for (index, &height) in heights.iter().enumerate() {
        if kept.last().is_none_or(|&last| height - heights[last] >= min_gap) {
            kept.push(index);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn group(year: i32, month: u32, start: usize) -> DateGroup {
        DateGroup {
            day: NaiveDate::from_ymd_opt(year, month, 1).unwrap(),
            start,
            photos: 1,
            videos: 0,
        }
    }

    #[test]
    fn req_006_16_marks_where_each_year_starts_in_either_order() {
        let newest_first = [
            group(2024, 5, 0),
            group(2024, 1, 7),
            group(2023, 12, 9),
            group(2020, 3, 30),
        ];
        assert_eq!(
            year_marks(&newest_first),
            [
                YearMark {
                    year: 2024,
                    first_item: 0
                },
                YearMark {
                    year: 2023,
                    first_item: 9
                },
                YearMark {
                    year: 2020,
                    first_item: 30
                },
            ]
        );
        let oldest_first = [group(2020, 3, 0), group(2023, 12, 21), group(2024, 1, 22)];
        assert_eq!(
            year_marks(&oldest_first)
                .iter()
                .map(|mark| mark.year)
                .collect::<Vec<_>>(),
            [2020, 2023, 2024]
        );
        assert_eq!(year_marks(&[]), []);
    }

    const TRACK: ScrollTrack = ScrollTrack {
        top: 100.0,
        bottom: 900.0,
        content: 8000.0,
        view: 800.0,
        min_handle: 32.0,
    };

    #[test]
    fn req_006_16_places_a_year_where_the_thumb_is_when_scrolled_there() {
        // A thumb of 800 * 800 / 8000 = 80 travels from 100 to 820.
        assert_eq!(TRACK.max_offset(), 7200.0);
        assert_eq!(TRACK.thumb_center(0.0), 140.0);
        assert_eq!(TRACK.thumb_center(3600.0), 500.0);
        assert_eq!(TRACK.thumb_center(7200.0), 860.0);
        // Past the end, as for a year that starts on the last screen.
        assert_eq!(TRACK.thumb_center(7900.0), 860.0);
    }

    #[test]
    fn req_006_19_the_offset_at_a_height_undoes_the_thumbs_center() {
        for offset in [0.0, 1234.0, 3600.0, 7200.0] {
            assert!(
                (TRACK.offset_at(TRACK.thumb_center(offset)) - offset).abs() < 1e-9,
                "{offset}"
            );
        }
        // Above and below the thumb's travel: the ends.
        assert_eq!(TRACK.offset_at(100.0), 0.0);
        assert_eq!(TRACK.offset_at(900.0), 7200.0);
        // Content that fits doesn't scroll.
        let fits = ScrollTrack {
            content: 500.0,
            ..TRACK
        };
        assert_eq!(fits.offset_at(500.0), 0.0);
    }

    #[test]
    fn req_006_16_a_long_gallery_has_the_shortest_thumb() {
        let long = ScrollTrack {
            content: 8_000_000.0,
            ..TRACK
        };
        assert_eq!(long.thumb_center(0.0), 116.0);
        assert_eq!(long.thumb_center(long.max_offset()), 884.0);
    }

    #[test]
    fn req_006_16_drops_years_too_close_together() {
        assert_eq!(spread(&[0.0, 30.0, 40.0, 49.0, 50.0, 200.0], 20.0), [0, 1, 4, 5]);
        assert_eq!(spread(&[], 20.0), Vec::<usize>::new());
        // Years on the last screen all sit at the bottom of the track.
        assert_eq!(spread(&[860.0, 860.0, 860.0], 20.0), [0]);
    }
}
