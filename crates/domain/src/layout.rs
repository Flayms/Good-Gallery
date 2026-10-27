//! Gallery layout (spec 006, 016): every tile and group header positioned up front, so the scroll length is exact
//! and the tiles in view are found by binary search. A port of the Electron version's `gallery-layout.ts`,
//! `justified.ts` and `masonry.ts`.

/// Panoramas and tall strips are clamped, so a single tile can't dominate a column.
const MIN_ASPECT: f64 = 0.3;
const MAX_ASPECT: f64 = 3.0;
/// Justified rows are this share of a Masonry column tall, so landscape photos keep about that width.
const ROW_HEIGHT_RATIO: f64 = 0.75;

/// Tile height / width, clamped to 1:3 … 3:1; a square when the dimensions are unknown (REQ-006-02).
pub fn tile_aspect(width: Option<i64>, height: Option<i64>) -> f64 {
    match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (h as f64 / w as f64).clamp(MIN_ASPECT, MAX_ASPECT),
        _ => 1.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileView {
    Masonry,
    Justified,
    Grid,
}

/// Width of a Masonry or Grid column: the container minus the gaps, in whole pixels.
pub fn column_width(container_width: f64, columns: usize, gap: f64) -> f64 {
    ((container_width - gap * (columns as f64 - 1.0)) / columns as f64)
        .floor()
        .max(0.0)
}

/// Height of a tile in a column, from its height / width aspect.
pub fn tile_height(aspect: f64, column_width: f64) -> f64 {
    (column_width * aspect).round()
}

pub struct LayoutOptions<'a> {
    pub view: TileView,
    /// Height / width of each item.
    pub aspects: &'a [f64],
    /// Index of each group's first item, sorted, the first at 0; each gets a header. Empty for one group without a
    /// header.
    pub groups: &'a [usize],
    pub width: f64,
    /// Zoom level: Masonry and Grid columns, and the Justified row height derived from them.
    pub columns: usize,
    pub gap: f64,
    pub header_height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Header {
    pub group: usize,
    pub top: f64,
}

/// Positions of all items and group headers, in pixels from the top of the content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TileLayout {
    /// Never decreases with the index, so the items in view can be found by binary search.
    pub top: Vec<f64>,
    pub left: Vec<f64>,
    pub width: Vec<f64>,
    pub height: Vec<f64>,
    /// Sorted by `top`.
    pub headers: Vec<Header>,
    pub header_height: f64,
    /// Height of the tallest item, to look back from a position for tiles that start above it.
    pub max_height: f64,
    pub total_height: f64,
}

/// Lays out the items of each group under its header: Masonry places each tile into the currently shortest column,
/// Grid does the same with squares, Justified fills rows of about equal height. Every group starts level below the
/// end of the previous one (REQ-016-05).
pub fn tile_layout(options: &LayoutOptions) -> TileLayout {
    let count = options.aspects.len();
    let mut layout = TileLayout {
        top: vec![0.0; count],
        left: vec![0.0; count],
        width: vec![0.0; count],
        height: vec![0.0; count],
        header_height: options.header_height,
        ..Default::default()
    };
    let columns = options.columns.max(1);
    let col_width = column_width(options.width, columns, options.gap);
    if col_width <= 0.0 {
        return layout;
    }
    let ranges: Vec<(usize, usize)> = if options.groups.is_empty() {
        vec![(0, count)]
    } else {
        options
            .groups
            .iter()
            .enumerate()
            .map(|(i, &start)| (start, options.groups.get(i + 1).copied().unwrap_or(count)))
            .collect()
    };
    let mut y = 0.0;
    for (group, &range) in ranges.iter().enumerate() {
        if !options.groups.is_empty() {
            layout.headers.push(Header { group, top: y });
            y += options.header_height;
        }
        let bottom = match options.view {
            TileView::Justified => {
                let target = (col_width * ROW_HEIGHT_RATIO).round();
                place_rows(
                    &mut layout,
                    range,
                    options.aspects,
                    options.width,
                    target,
                    options.gap,
                    y,
                )
            }
            TileView::Grid => place_columns(&mut layout, range, |_| 1.0, columns, col_width, options.gap, y),
            TileView::Masonry => place_columns(
                &mut layout,
                range,
                |i| options.aspects[i],
                columns,
                col_width,
                options.gap,
                y,
            ),
        };
        y = bottom + if group + 1 < ranges.len() { options.gap } else { 0.0 };
    }
    layout.total_height = y;
    layout.max_height = layout.height.iter().copied().fold(0.0, f64::max);
    layout
}

/// Masonry placement of the range from `y` on; returns the bottom of the tallest column.
fn place_columns(
    layout: &mut TileLayout,
    (start, end): (usize, usize),
    aspect_at: impl Fn(usize) -> f64,
    columns: usize,
    col_width: f64,
    gap: f64,
    y: f64,
) -> f64 {
    // Column ends including the gap after the last tile.
    let mut ends = vec![y; columns];
    for index in start..end {
        let mut column = 0;
        for c in 1..columns {
            if ends[c] < ends[column] {
                column = c;
            }
        }
        let height = tile_height(aspect_at(index), col_width);
        let top = ends[column];
        layout.top[index] = top;
        layout.left[index] = column as f64 * (col_width + gap);
        layout.width[index] = col_width;
        layout.height[index] = height;
        ends[column] = top + height + gap;
    }
    if start < end {
        ends.iter().copied().fold(f64::MIN, f64::max) - gap
    } else {
        y
    }
}

/// Justified rows of the range from `y` on; returns the bottom of the last row.
fn place_rows(
    layout: &mut TileLayout,
    (start, end): (usize, usize),
    aspects: &[f64],
    width: f64,
    target_height: f64,
    gap: f64,
    y: f64,
) -> f64 {
    let group = &aspects[start..end];
    let mut top = y;
    for row in justified_rows(group, width, target_height, gap) {
        let mut left = 0.0;
        for (offset, item_width) in row_item_widths(&row, group, width, gap).into_iter().enumerate() {
            let index = start + row.start + offset;
            layout.top[index] = top;
            layout.left[index] = left;
            layout.width[index] = item_width;
            layout.height[index] = row.height;
            left += item_width + gap;
        }
        top += row.height + gap;
    }
    if top > y { top - gap } else { y }
}

/// A Justified row: items `start..end`, all `height` pixels tall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JustifiedRow {
    pub start: usize,
    pub end: usize,
    pub height: f64,
    /// Whether the row spans the whole width; only the last row may not.
    pub full: bool,
}

/// Splits the items into rows that fill `container_width` exactly, each about `target_height` tall: items are added
/// at the target height until the row is full, then the row is scaled down to fit. The last row keeps the target
/// height instead of being stretched (REQ-006-03).
pub fn justified_rows(aspects: &[f64], container_width: f64, target_height: f64, gap: f64) -> Vec<JustifiedRow> {
    if container_width <= 0.0 || target_height <= 0.0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut start = 0;
    // Sum of width / height of the items in the current row.
    let mut ratio_sum = 0.0;
    for (index, aspect) in aspects.iter().enumerate() {
        ratio_sum += 1.0 / aspect;
        let available = container_width - gap * (index - start) as f64;
        if ratio_sum * target_height >= available {
            rows.push(JustifiedRow {
                start,
                end: index + 1,
                height: (available / ratio_sum).round().max(1.0),
                full: true,
            });
            start = index + 1;
            ratio_sum = 0.0;
        }
    }
    if start < aspects.len() {
        rows.push(JustifiedRow {
            start,
            end: aspects.len(),
            height: target_height,
            full: false,
        });
    }
    rows
}

/// Whole-pixel widths of the row's items; a full row's rounding remainder goes to its last item, so it ends flush.
pub fn row_item_widths(row: &JustifiedRow, aspects: &[f64], container_width: f64, gap: f64) -> Vec<f64> {
    let mut widths: Vec<f64> = aspects[row.start..row.end]
        .iter()
        .map(|aspect| (row.height / aspect).round())
        .collect();
    if row.full
        && let Some(last) = widths.len().checked_sub(1)
    {
        let used: f64 = widths.iter().sum::<f64>() + gap * last as f64;
        widths[last] = (widths[last] + container_width - used).max(1.0);
    }
    widths
}

/// First index in `0..length` for which `test` holds, or `length`; `test` must be monotonic.
fn lower_bound(length: usize, test: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, length);
    while low < high {
        let mid = (low + high) / 2;
        if test(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

impl TileLayout {
    /// Items `start..end` that may intersect `top..bottom`: all that do, and a few tiles above it.
    pub fn items_in_view(&self, top: f64, bottom: f64) -> (usize, usize) {
        // Before the width is known no item is placed: all would sit at 0 and be "in view" at once.
        if self.total_height == 0.0 {
            return (0, 0);
        }
        let count = self.top.len();
        let start = lower_bound(count, |i| self.top[i] >= top - self.max_height);
        let end = lower_bound(count, |i| self.top[i] >= bottom);
        (start.min(end), end)
    }

    /// Headers `start..end` that intersect `top..bottom`.
    pub fn headers_in_view(&self, top: f64, bottom: f64) -> (usize, usize) {
        let headers = &self.headers;
        let start = lower_bound(headers.len(), |i| headers[i].top + self.header_height > top);
        let end = lower_bound(headers.len(), |i| headers[i].top >= bottom);
        (start.min(end), end)
    }

    /// First item that starts at or below `y`, else the last item.
    pub fn first_item_from(&self, y: f64) -> usize {
        let count = self.top.len();
        lower_bound(count, |i| self.top[i] >= y).min(count.saturating_sub(1))
    }

    /// Top of item `index` when scrolled to the start: with its group's header if it is the group's first item.
    pub fn start_of(&self, groups: &[usize], index: usize) -> f64 {
        let top = self.top.get(index).copied().unwrap_or(0.0);
        match group_of_item(groups, index) {
            Some(group) if groups[group] == index => top - self.header_height,
            _ => top,
        }
    }
}

/// The group containing item `index` (groups are sorted and contiguous), or `None` without groups.
pub fn group_of_item(groups: &[usize], index: usize) -> Option<usize> {
    lower_bound(groups.len(), |g| groups[g] > index).checked_sub(1)
}

/// An entry of the List view: a group header or a media item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListEntry {
    Group(usize),
    Item(usize),
}

/// The entry at list position `index`, where each group's header comes before its first item.
pub fn list_entry_at(groups: &[usize], index: usize) -> ListEntry {
    // Header `g` is at position `start + g`.
    match lower_bound(groups.len(), |g| groups[g] + g > index).checked_sub(1) {
        None => ListEntry::Item(index),
        Some(group) if groups[group] + group == index => ListEntry::Group(group),
        Some(group) => ListEntry::Item(index - group - 1),
    }
}

/// List position of item `index`.
pub fn list_index_of_item(groups: &[usize], index: usize) -> usize {
    match group_of_item(groups, index) {
        Some(group) => index + group + 1,
        None => index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options<'a>(view: TileView, aspects: &'a [f64], groups: &'a [usize]) -> LayoutOptions<'a> {
        LayoutOptions {
            view,
            aspects,
            groups,
            width: 316.0,
            columns: 3,
            gap: 8.0,
            header_height: 40.0,
        }
    }

    /// Boxes of the items as `[left, top, width, height]`.
    fn boxes(layout: &TileLayout) -> Vec<[f64; 4]> {
        (0..layout.top.len())
            .map(|i| [layout.left[i], layout.top[i], layout.width[i], layout.height[i]])
            .collect()
    }

    #[test]
    fn req_006_02_clamps_aspects_and_treats_unknown_sizes_as_squares() {
        assert_eq!(tile_aspect(Some(400), Some(300)), 0.75);
        assert_eq!(tile_aspect(Some(1000), Some(100)), 0.3);
        assert_eq!(tile_aspect(Some(100), Some(1000)), 3.0);
        assert_eq!(tile_aspect(None, Some(10)), 1.0);
        assert_eq!(tile_aspect(Some(0), Some(10)), 1.0);
    }

    #[test]
    fn req_006_02_splits_columns_and_scales_tiles() {
        assert_eq!(column_width(1000.0, 4, 8.0), 244.0);
        assert_eq!(column_width(10.0, 4, 8.0), 0.0);
        assert_eq!(tile_height(0.75, 200.0), 150.0);
    }

    #[test]
    fn req_006_02_places_each_masonry_tile_into_the_shortest_column() {
        let layout = tile_layout(&options(TileView::Masonry, &[1.0, 0.5, 2.0, 1.0, 1.0], &[]));

        // Columns are 100 px wide: heights 100, 50, 200; then the 50 px column, then the 100 px one.
        assert_eq!(
            boxes(&layout),
            [
                [0.0, 0.0, 100.0, 100.0],
                [108.0, 0.0, 100.0, 50.0],
                [216.0, 0.0, 100.0, 200.0],
                [108.0, 58.0, 100.0, 100.0],
                [0.0, 108.0, 100.0, 100.0],
            ]
        );
        assert_eq!(layout.total_height, 208.0);
        assert!(layout.headers.is_empty());
    }

    #[test]
    fn req_016_05_starts_the_columns_of_each_group_level_below_its_header() {
        let layout = tile_layout(&options(TileView::Masonry, &[1.0, 2.0, 0.5], &[0, 2]));

        assert_eq!(
            layout.headers,
            [
                Header { group: 0, top: 0.0 },
                // Below the tallest column of the first group (40 + 200) and a gap.
                Header { group: 1, top: 248.0 },
            ]
        );
        assert_eq!(
            boxes(&layout),
            [
                [0.0, 40.0, 100.0, 100.0],
                [108.0, 40.0, 100.0, 200.0],
                [0.0, 288.0, 100.0, 50.0]
            ]
        );
        assert_eq!(layout.total_height, 338.0);
    }

    #[test]
    fn req_006_04_req_016_05_lays_out_the_grid_as_squares_each_group_starting_a_new_row() {
        let layout = tile_layout(&options(TileView::Grid, &[3.0; 4], &[0, 1]));

        assert_eq!(
            boxes(&layout),
            [
                [0.0, 40.0, 100.0, 100.0],
                [0.0, 188.0, 100.0, 100.0],
                [108.0, 188.0, 100.0, 100.0],
                [216.0, 188.0, 100.0, 100.0],
            ]
        );
    }

    #[test]
    fn req_006_03_req_016_05_fills_justified_rows_per_group_and_keeps_the_last_row_unstretched() {
        // Target row height: 75 (3/4 of a 100 px column).
        let layout = tile_layout(&options(TileView::Justified, &[1.0; 5], &[0, 1]));
        let boxes = boxes(&layout);

        // First group: one item that doesn't fill the row, at the target height.
        assert_eq!(boxes[0], [0.0, 40.0, 75.0, 75.0]);
        // Second group: four squares fill a row (4 × 75 + 3 × 8 > 316), scaled down to fit, flush on the right.
        let row = &boxes[1..];
        assert!(row.iter().all(|b| b[1] == 163.0 && b[3] == 73.0), "{row:?}");
        assert_eq!(row[3][0] + row[3][2], 316.0);
    }

    #[test]
    fn req_016_06_keeps_item_tops_in_order_in_every_view() {
        let mut seed: u64 = 7;
        let mut random = || {
            seed = (seed * 16_807) % 2_147_483_647;
            seed as f64 / 2_147_483_647.0
        };
        let aspects: Vec<f64> = (0..500).map(|_| 0.3 + random() * 2.7).collect();
        let groups = [0, 3, 4, 50, 51, 200, 499];
        for view in [TileView::Masonry, TileView::Justified, TileView::Grid] {
            let layout = tile_layout(&LayoutOptions {
                columns: 5,
                ..options(view, &aspects, &groups)
            });
            assert!(layout.top.windows(2).all(|w| w[1] >= w[0]), "{view:?}");
        }
    }

    #[test]
    fn req_006_03_fills_rows_to_the_container_width_and_keeps_the_last_row_at_the_target_height() {
        // Landscape 4:3 at 150 px is 200 px wide: four fit into 840 px with gaps, the fifth overflows the row.
        let aspects = [0.75; 6];
        let rows = justified_rows(&aspects, 840.0, 150.0, 10.0);

        assert_eq!(
            rows,
            [
                JustifiedRow {
                    start: 0,
                    end: 5,
                    height: 120.0,
                    full: true
                },
                JustifiedRow {
                    start: 5,
                    end: 6,
                    height: 150.0,
                    full: false
                },
            ]
        );
        let widths = row_item_widths(&rows[0], &aspects, 840.0, 10.0);
        assert_eq!(widths.iter().sum::<f64>() + 10.0 * (widths.len() - 1) as f64, 840.0);
    }

    /// REQ-006-03: rounding leftovers go to the last item, so a full row ends exactly at the right edge.
    #[test]
    fn req_006_03_ends_full_rows_flush() {
        let row = JustifiedRow {
            start: 0,
            end: 3,
            height: 100.0,
            full: true,
        };
        assert_eq!(row_item_widths(&row, &[0.75; 3], 420.0, 8.0), [133.0, 133.0, 138.0]);
        let last = JustifiedRow { full: false, ..row };
        assert_eq!(row_item_widths(&last, &[0.75; 3], 420.0, 8.0), [133.0, 133.0, 133.0]);
    }

    #[test]
    fn req_006_03_gives_an_over_wide_panorama_a_row_of_its_own() {
        let rows = justified_rows(&[0.3, 1.0], 400.0, 200.0, 8.0);

        assert_eq!(
            rows[0],
            JustifiedRow {
                start: 0,
                end: 1,
                height: 120.0,
                full: true
            }
        );
        assert_eq!((rows[1].start, rows[1].end, rows[1].full), (1, 2, false));
    }

    #[test]
    fn has_no_rows_without_items_or_width() {
        assert!(justified_rows(&[], 800.0, 200.0, 8.0).is_empty());
        assert!(justified_rows(&[1.0; 3], 0.0, 200.0, 8.0).is_empty());
    }

    /// Tops: 40, 40, 40 | header 248 | 288, 288.
    fn grouped() -> TileLayout {
        tile_layout(&options(TileView::Masonry, &[1.0, 0.5, 2.0, 1.0, 1.0], &[0, 3]))
    }

    #[test]
    fn req_016_06_finds_the_items_intersecting_a_range_including_tall_ones_that_start_above_it() {
        let layout = grouped();
        let (start, end) = layout.items_in_view(200.0, 260.0);
        assert!(start <= 2);
        assert_eq!(end, 3);
        assert_eq!(layout.items_in_view(300.0, 400.0), (3, 5));
    }

    #[test]
    fn req_006_08_finds_no_items_before_the_width_is_known() {
        let aspects = vec![1.0; 20_000];
        let layout = tile_layout(&LayoutOptions {
            width: 0.0,
            ..options(TileView::Masonry, &aspects, &[])
        });
        assert_eq!(layout.items_in_view(-600.0, 600.0), (0, 0));
    }

    #[test]
    fn req_016_06_finds_headers_and_the_first_item_from_a_position() {
        let layout = grouped();
        assert_eq!(layout.headers_in_view(0.0, 100.0), (0, 1));
        assert_eq!(layout.headers_in_view(100.0, 300.0), (1, 2));
        assert_eq!(layout.first_item_from(0.0), 0);
        assert_eq!(layout.first_item_from(41.0), 3);
        assert_eq!(layout.first_item_from(1000.0), 4);
        assert_eq!(layout.start_of(&[0, 3], 3), 248.0);
        assert_eq!(layout.start_of(&[0, 3], 4), 288.0);
    }

    #[test]
    fn req_016_05_interleaves_group_headers_with_the_list_items() {
        let groups = [0, 2, 3];
        // Entries: header 0, item 0, item 1, header 1, item 2, header 2, item 3, item 4.
        let entries: Vec<ListEntry> = (0..8).map(|i| list_entry_at(&groups, i)).collect();
        assert_eq!(
            entries,
            [
                ListEntry::Group(0),
                ListEntry::Item(0),
                ListEntry::Item(1),
                ListEntry::Group(1),
                ListEntry::Item(2),
                ListEntry::Group(2),
                ListEntry::Item(3),
                ListEntry::Item(4),
            ]
        );
        let positions: Vec<usize> = (0..5).map(|item| list_index_of_item(&groups, item)).collect();
        assert_eq!(positions, [1, 2, 4, 6, 7]);
        assert_eq!(list_entry_at(&[], 3), ListEntry::Item(3));
        assert_eq!(list_index_of_item(&[], 3), 3);
    }
}
