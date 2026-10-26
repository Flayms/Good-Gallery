//! Quick port of `gallery-layout.ts`, `justified.ts` and `masonry.ts` (no groups). The tested port is M6.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Justified,
    Masonry,
    Grid,
}

/// Row height relative to the masonry column width at the same zoom.
const ROW_HEIGHT_RATIO: f32 = 0.75;

#[derive(Default)]
pub struct Layout {
    pub top: Vec<f32>,
    pub left: Vec<f32>,
    pub width: Vec<f32>,
    pub height: Vec<f32>,
    pub max_height: f32,
    pub total_height: f32,
}

pub fn column_width(width: f32, columns: usize, gap: f32) -> f32 {
    ((width - gap * (columns as f32 - 1.0)) / columns as f32)
        .floor()
        .max(0.0)
}

/// `aspects` are height / width.
pub fn tile_layout(view: View, aspects: &[f32], width: f32, columns: usize, gap: f32) -> Layout {
    let n = aspects.len();
    let mut l = Layout {
        top: vec![0.0; n],
        left: vec![0.0; n],
        width: vec![0.0; n],
        height: vec![0.0; n],
        ..Default::default()
    };
    let col_width = column_width(width, columns, gap);
    if col_width <= 0.0 {
        return l;
    }
    l.total_height = match view {
        View::Justified => place_rows(
            &mut l,
            aspects,
            width,
            (col_width * ROW_HEIGHT_RATIO).round(),
            gap,
        ),
        View::Masonry => place_columns(&mut l, aspects, columns, col_width, gap, false),
        View::Grid => place_columns(&mut l, aspects, columns, col_width, gap, true),
    };
    l.max_height = l.height.iter().copied().fold(0.0, f32::max);
    l
}

fn place_columns(
    l: &mut Layout,
    aspects: &[f32],
    columns: usize,
    col_width: f32,
    gap: f32,
    square: bool,
) -> f32 {
    let mut ends = vec![0.0f32; columns];
    for (i, &aspect) in aspects.iter().enumerate() {
        let mut column = 0;
        for c in 1..columns {
            if ends[c] < ends[column] {
                column = c;
            }
        }
        let height = (col_width * if square { 1.0 } else { aspect }).round();
        l.top[i] = ends[column];
        l.left[i] = column as f32 * (col_width + gap);
        l.width[i] = col_width;
        l.height[i] = height;
        ends[column] += height + gap;
    }
    if aspects.is_empty() {
        0.0
    } else {
        ends.iter().copied().fold(0.0, f32::max) - gap
    }
}

fn place_rows(l: &mut Layout, aspects: &[f32], width: f32, target: f32, gap: f32) -> f32 {
    if target <= 0.0 {
        return 0.0;
    }
    let mut top = 0.0;
    let mut start = 0;
    let mut ratio_sum = 0.0;
    let emit = |l: &mut Layout, start: usize, end: usize, height: f32, full: bool, top: f32| {
        let mut left = 0.0;
        let mut used = 0.0;
        for i in start..end {
            let w = (height / aspects[i]).round();
            l.top[i] = top;
            l.left[i] = left;
            l.width[i] = w;
            l.height[i] = height;
            left += w + gap;
            used += w;
        }
        // A full row's rounding remainder goes to its last item so it ends flush.
        if full && end > start {
            let used = used + gap * (end - start - 1) as f32;
            l.width[end - 1] = (l.width[end - 1] + width - used).max(1.0);
        }
    };
    for i in 0..aspects.len() {
        ratio_sum += 1.0 / aspects[i];
        let available = width - gap * (i - start) as f32;
        if ratio_sum * target >= available {
            let height = (available / ratio_sum).round().max(1.0);
            emit(l, start, i + 1, height, true, top);
            top += height + gap;
            start = i + 1;
            ratio_sum = 0.0;
        }
    }
    if start < aspects.len() {
        emit(l, start, aspects.len(), target, false, top);
        top += target + gap;
    }
    if top > 0.0 { top - gap } else { 0.0 }
}

fn lower_bound(len: usize, test: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, len);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if test(mid) { hi = mid } else { lo = mid + 1 }
    }
    lo
}

/// First item that starts at or below `y`, else the last item (`firstItemFrom` in the reference).
pub fn first_item_from(l: &Layout, y: f32) -> usize {
    let n = l.top.len();
    lower_bound(n, |i| l.top[i] >= y).min(n.saturating_sub(1))
}

/// Items `[start, end)` that may intersect `[top, bottom)`.
pub fn items_in_view(l: &Layout, top: f32, bottom: f32) -> (usize, usize) {
    if l.total_height == 0.0 {
        return (0, 0);
    }
    let n = l.top.len();
    let start = lower_bound(n, |i| l.top[i] >= top - l.max_height);
    let end = lower_bound(n, |i| l.top[i] >= bottom);
    (start.min(end), end)
}
