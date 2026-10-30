//! Photo editing (spec 021): an edit is a [`Recipe`] of adjustments, rendered per pixel by [`Params::pixel`] from the
//! unedited original. The viewer's preview runs the same formulas on the GPU (`app::edit`'s shader mirrors this file);
//! saving runs them here. Also the undo [`History`], the name of a saved copy and where backups of originals go.

use serde::{Deserialize, Serialize};

/// The recipe format. Recipes of a newer version than this are not read (a newer app wrote them).
pub const RECIPE_VERSION: u32 = 1;

/// Where originals replaced by an edit are kept, relative to their library (a housekeeping folder, so never indexed).
pub const BACKUP_DIR: &str = ".good-gallery/originals";

/// Samples per channel of the composed curve tables; looked up with linear interpolation.
pub const CURVE_SAMPLES: usize = 256;

/// At most this many points per curve, endpoints included.
pub const MAX_CURVE_POINTS: usize = 16;

/// How far the white balance sliders reach, in stops between red and blue (temperature) or green and the others
/// (tint).
const WHITE_BALANCE_STOPS: f32 = 1.0;

/// What saving does to a file's date modified: the setting _When saving edits_ (REQ-021-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MtimeOnSave {
    /// The original's date modified, so photos without a date taken keep their place.
    #[default]
    Keep,
    /// The time of saving.
    Update,
}

impl MtimeOnSave {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Update => "update",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "keep" => Some(Self::Keep),
            "update" => Some(Self::Update),
            _ => None,
        }
    }
}

/// The edit a file was saved with, read from its XMP (`gg:EditRecipe`, `gg:EditSource`) and kept in the index
/// (REQ-021-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedEdit {
    /// The recipe's JSON ([`Recipe::to_json`]).
    pub recipe: String,
    /// The unedited original, relative to the library: the backup after _Save_, the source after _Save as copy_.
    pub source: String,
}

impl SavedEdit {
    /// From a file's tags: both present, a recipe this version reads, and a source inside the library (REQ-011-08).
    pub fn parse(recipe: Option<&str>, source: Option<&str>) -> Option<Self> {
        let (recipe, source) = (recipe?, source?);
        (Recipe::from_json(recipe).is_some() && crate::media::is_safe_rel_path(source)).then(|| Self {
            recipe: recipe.to_owned(),
            source: source.to_owned(),
        })
    }

    pub fn recipe(&self) -> Recipe {
        Recipe::from_json(&self.recipe).unwrap_or_default()
    }

    /// Whether the edit replaced its original, which is then kept as a backup (REQ-021-12).
    pub fn replaced_original(&self, rel_path: &str) -> bool {
        self.source == backup_rel_path(rel_path)
    }
}

/// Whether a photo file can be edited, by extension (REQ-021-01): JPEG, PNG and WebP. GIFs may be animated, AVIF has
/// no encoder here.
pub fn is_editable(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".webp"]
        .iter()
        .any(|ext| lower.len() > ext.len() && lower.ends_with(ext))
}

/// Where the unedited original of `rel_path` is kept once an edit replaced it (REQ-021-10).
pub fn backup_rel_path(rel_path: &str) -> String {
    format!("{BACKUP_DIR}/{rel_path}")
}

/// The file name of an edited copy of `file_name`: `IMG_1 (edited).jpg`, numbered from 2 while `taken` says a name is
/// in use (REQ-021-09).
pub fn copy_name(file_name: &str, taken: impl Fn(&str) -> bool) -> String {
    let (stem, ext) = match file_name.rfind('.') {
        Some(dot) if dot > 0 => file_name.split_at(dot),
        _ => (file_name, ""),
    };
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem} (edited){ext}")
        } else {
            format!("{stem} (edited {n}){ext}")
        };
        if !taken(&name) {
            return name;
        }
        n += 1;
    }
}

/// One tone curve: points from black to white, sorted by input (REQ-021-04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
}

impl Default for Curve {
    fn default() -> Self {
        Self {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        }
    }
}

impl Curve {
    /// A straight line from black to white.
    pub fn is_identity(&self) -> bool {
        self.points.iter().all(|[x, y]| (x - y).abs() < 1e-6)
    }

    /// Endpoints at 0 and 1, values in 0–1, sorted, at most [`MAX_CURVE_POINTS`] (recipes from files may be hostile).
    fn sanitize(&mut self) {
        let mut points: Vec<[f32; 2]> = self
            .points
            .iter()
            .filter(|[x, y]| x.is_finite() && y.is_finite())
            .map(|[x, y]| [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)])
            .take(MAX_CURVE_POINTS)
            .collect();
        points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-4);
        if points.len() < 2 {
            points = Self::default().points;
        }
        self.points = points;
    }

    /// The curve at `x`: a monotone cubic through the points (Fritsch–Carlson), so it never overshoots them; flat
    /// beyond the endpoints.
    pub fn eval(&self, x: f32) -> f32 {
        let p = &self.points;
        let n = p.len();
        if n == 0 {
            return x;
        }
        if x <= p[0][0] {
            return p[0][1];
        }
        if x >= p[n - 1][0] {
            return p[n - 1][1];
        }
        let slopes = self.tangents();
        let i = p.windows(2).position(|w| x < w[1][0]).unwrap_or(n - 2);
        let ([x0, y0], [x1, y1]) = (p[i], p[i + 1]);
        let h = x1 - x0;
        let t = (x - x0) / h;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * h * slopes[i]
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * h * slopes[i + 1];
        y.clamp(0.0, 1.0)
    }

    fn tangents(&self) -> Vec<f32> {
        let p = &self.points;
        let n = p.len();
        let secants: Vec<f32> = p
            .windows(2)
            .map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0]))
            .collect();
        let mut m = vec![0.0; n];
        m[0] = secants[0];
        m[n - 1] = secants[n - 2];
        for i in 1..n - 1 {
            m[i] = if secants[i - 1] * secants[i] <= 0.0 {
                0.0
            } else {
                (secants[i - 1] + secants[i]) / 2.0
            };
        }
        for i in 0..n - 1 {
            if secants[i] == 0.0 {
                m[i] = 0.0;
                m[i + 1] = 0.0;
                continue;
            }
            let (a, b) = (m[i] / secants[i], m[i + 1] / secants[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let tau = 3.0 / s.sqrt();
                m[i] = tau * a * secants[i];
                m[i + 1] = tau * b * secants[i];
            }
        }
        m
    }

    /// Adds a point at `x` (if there is room and none is that close) and returns its index.
    pub fn add_point(&mut self, x: f32, y: f32) -> Option<usize> {
        let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        if self.points.len() >= MAX_CURVE_POINTS || self.points.iter().any(|p| (p[0] - x).abs() < 0.02) {
            return None;
        }
        let at = self.points.iter().position(|p| p[0] > x).unwrap_or(self.points.len());
        self.points.insert(at, [x, y]);
        Some(at)
    }

    /// Moves point `i` to `(x, y)`, kept between its neighbours and inside 0–1.
    pub fn move_point(&mut self, i: usize, x: f32, y: f32) {
        let n = self.points.len();
        if i >= n {
            return;
        }
        let low = if i == 0 { 0.0 } else { self.points[i - 1][0] + 0.01 };
        let high = if i + 1 == n { 1.0 } else { self.points[i + 1][0] - 0.01 };
        self.points[i] = [x.clamp(low, high.max(low)), y.clamp(0.0, 1.0)];
    }

    /// Removes point `i`, unless it is one of the endpoints.
    pub fn remove_point(&mut self, i: usize) -> bool {
        if i == 0 || i + 1 >= self.points.len() {
            return false;
        }
        self.points.remove(i);
        true
    }
}

/// Which tone curve: all channels together, or one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    All,
    Red,
    Green,
    Blue,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Self::All, Self::Red, Self::Green, Self::Blue];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "RGB",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curves {
    pub all: Curve,
    pub red: Curve,
    pub green: Curve,
    pub blue: Curve,
}

impl Curves {
    pub fn get(&self, channel: Channel) -> &Curve {
        match channel {
            Channel::All => &self.all,
            Channel::Red => &self.red,
            Channel::Green => &self.green,
            Channel::Blue => &self.blue,
        }
    }

    pub fn get_mut(&mut self, channel: Channel) -> &mut Curve {
        match channel {
            Channel::All => &mut self.all,
            Channel::Red => &mut self.red,
            Channel::Green => &mut self.green,
            Channel::Blue => &mut self.blue,
        }
    }

    pub fn is_identity(&self) -> bool {
        Channel::ALL.iter().all(|&channel| self.get(channel).is_identity())
    }
}

/// An edit's adjustments (REQ-021-03 … REQ-021-06). Sliders run from −100 to +100 unless noted; every one is neutral at
/// its default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Recipe {
    pub version: u32,
    /// EV, −5 to +5.
    pub exposure: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub curves: Curves,
    pub temperature: f32,
    pub tint: f32,
    pub vibrance: f32,
    pub saturation: f32,
    /// 0 to 100.
    pub grain: f32,
    /// 0 to 100.
    pub grain_size: f32,
    pub vignette: f32,
    /// 0 to 100.
    pub vignette_midpoint: f32,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            version: RECIPE_VERSION,
            exposure: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            curves: Curves::default(),
            temperature: 0.0,
            tint: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            grain: 0.0,
            grain_size: 25.0,
            vignette: 0.0,
            vignette_midpoint: 50.0,
        }
    }
}

impl Recipe {
    /// Whether it leaves every pixel as it is.
    pub fn is_neutral(&self) -> bool {
        Adjustment::ALL
            .iter()
            .filter(|adjustment| adjustment.changes_pixels())
            .all(|adjustment| adjustment.get(self) == adjustment.neutral())
            && self.curves.is_identity()
    }

    /// Compact JSON, as stored in a saved file (`gg:EditRecipe`) and the index.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A recipe from a file or the index, with every value brought into its range (REQ-011-10). `None` for invalid
    /// JSON and for recipes of a newer version.
    pub fn from_json(json: &str) -> Option<Self> {
        let mut recipe: Self = serde_json::from_str(json).ok()?;
        if recipe.version > RECIPE_VERSION {
            return None;
        }
        recipe.version = RECIPE_VERSION;
        for adjustment in Adjustment::ALL {
            let value = adjustment.get(&recipe);
            let (low, high) = adjustment.range();
            let value = if value.is_finite() {
                value.clamp(low, high)
            } else {
                adjustment.neutral()
            };
            adjustment.set(&mut recipe, value);
        }
        for channel in Channel::ALL {
            recipe.curves.get_mut(channel).sanitize();
        }
        Some(recipe)
    }
}

/// The groups of the adjustments panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Light,
    Curves,
    Color,
    Effects,
}

impl Section {
    pub const ALL: [Section; 4] = [Self::Light, Self::Curves, Self::Color, Self::Effects];

    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Curves => "Curves",
            Self::Color => "Color",
            Self::Effects => "Effects",
        }
    }
}

/// One slider of a [`Recipe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjustment {
    Exposure,
    Brightness,
    Contrast,
    Highlights,
    Shadows,
    Whites,
    Blacks,
    Temperature,
    Tint,
    Vibrance,
    Saturation,
    Grain,
    GrainSize,
    Vignette,
    VignetteMidpoint,
}

impl Adjustment {
    pub const ALL: [Adjustment; 15] = [
        Self::Exposure,
        Self::Brightness,
        Self::Contrast,
        Self::Highlights,
        Self::Shadows,
        Self::Whites,
        Self::Blacks,
        Self::Temperature,
        Self::Tint,
        Self::Vibrance,
        Self::Saturation,
        Self::Grain,
        Self::GrainSize,
        Self::Vignette,
        Self::VignetteMidpoint,
    ];

    pub fn section(self) -> Section {
        match self {
            Self::Exposure
            | Self::Brightness
            | Self::Contrast
            | Self::Highlights
            | Self::Shadows
            | Self::Whites
            | Self::Blacks => Section::Light,
            Self::Temperature | Self::Tint | Self::Vibrance | Self::Saturation => Section::Color,
            Self::Grain | Self::GrainSize | Self::Vignette | Self::VignetteMidpoint => Section::Effects,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Exposure => "Exposure",
            Self::Brightness => "Brightness",
            Self::Contrast => "Contrast",
            Self::Highlights => "Highlights",
            Self::Shadows => "Shadows",
            Self::Whites => "Whites",
            Self::Blacks => "Blacks",
            Self::Temperature => "Temperature",
            Self::Tint => "Tint",
            Self::Vibrance => "Vibrance",
            Self::Saturation => "Saturation",
            Self::Grain => "Grain",
            Self::GrainSize => "Grain size",
            Self::Vignette => "Vignette",
            Self::VignetteMidpoint => "Midpoint",
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Exposure => (-5.0, 5.0),
            Self::Grain | Self::GrainSize | Self::VignetteMidpoint => (0.0, 100.0),
            _ => (-100.0, 100.0),
        }
    }

    pub fn neutral(self) -> f32 {
        match self {
            Self::GrainSize => 25.0,
            Self::VignetteMidpoint => 50.0,
            _ => 0.0,
        }
    }

    /// False for the shape of an effect (grain size, vignette midpoint), which alone changes nothing.
    pub fn changes_pixels(self) -> bool {
        !matches!(self, Self::GrainSize | Self::VignetteMidpoint)
    }

    pub fn get(self, recipe: &Recipe) -> f32 {
        match self {
            Self::Exposure => recipe.exposure,
            Self::Brightness => recipe.brightness,
            Self::Contrast => recipe.contrast,
            Self::Highlights => recipe.highlights,
            Self::Shadows => recipe.shadows,
            Self::Whites => recipe.whites,
            Self::Blacks => recipe.blacks,
            Self::Temperature => recipe.temperature,
            Self::Tint => recipe.tint,
            Self::Vibrance => recipe.vibrance,
            Self::Saturation => recipe.saturation,
            Self::Grain => recipe.grain,
            Self::GrainSize => recipe.grain_size,
            Self::Vignette => recipe.vignette,
            Self::VignetteMidpoint => recipe.vignette_midpoint,
        }
    }

    pub fn set(self, recipe: &mut Recipe, value: f32) {
        let (low, high) = self.range();
        let value = value.clamp(low, high);
        let field = match self {
            Self::Exposure => &mut recipe.exposure,
            Self::Brightness => &mut recipe.brightness,
            Self::Contrast => &mut recipe.contrast,
            Self::Highlights => &mut recipe.highlights,
            Self::Shadows => &mut recipe.shadows,
            Self::Whites => &mut recipe.whites,
            Self::Blacks => &mut recipe.blacks,
            Self::Temperature => &mut recipe.temperature,
            Self::Tint => &mut recipe.tint,
            Self::Vibrance => &mut recipe.vibrance,
            Self::Saturation => &mut recipe.saturation,
            Self::Grain => &mut recipe.grain,
            Self::GrainSize => &mut recipe.grain_size,
            Self::Vignette => &mut recipe.vignette,
            Self::VignetteMidpoint => &mut recipe.vignette_midpoint,
        };
        *field = value;
    }
}

/// Undo and redo over recipes (REQ-021-07): one entry per finished change.
#[derive(Debug, Clone, PartialEq)]
pub struct History {
    states: Vec<Recipe>,
    at: usize,
}

/// Entries kept; older ones drop out.
const HISTORY_LIMIT: usize = 100;

impl History {
    pub fn new(recipe: Recipe) -> Self {
        Self {
            states: vec![recipe],
            at: 0,
        }
    }

    pub fn current(&self) -> &Recipe {
        &self.states[self.at]
    }

    /// Records `recipe` as the newest state, dropping what could have been redone; nothing for an unchanged recipe.
    pub fn push(&mut self, recipe: Recipe) {
        if recipe == *self.current() {
            return;
        }
        self.states.truncate(self.at + 1);
        self.states.push(recipe);
        if self.states.len() > HISTORY_LIMIT {
            self.states.remove(0);
        }
        self.at = self.states.len() - 1;
    }

    pub fn can_undo(&self) -> bool {
        self.at > 0
    }

    pub fn can_redo(&self) -> bool {
        self.at + 1 < self.states.len()
    }

    pub fn undo(&mut self) -> bool {
        let can = self.can_undo();
        if can {
            self.at -= 1;
        }
        can
    }

    pub fn redo(&mut self) -> bool {
        let can = self.can_redo();
        if can {
            self.at += 1;
        }
        can
    }
}

// --- Per-pixel formulas ---

/// A recipe turned into the numbers the formulas use. The GPU shader gets the same numbers (`app::edit`).
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// White balance gains on linear RGB.
    pub gains: [f32; 3],
    /// `2^EV`.
    pub exposure: f32,
    /// Black and white points of the perceptual luminance.
    pub black: f32,
    pub white: f32,
    /// The midtones' gamma, 1 for none.
    pub brightness: f32,
    pub shadows: f32,
    pub highlights: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub vignette: f32,
    pub vignette_start: f32,
    pub grain: f32,
    /// Grain cell size in pixels of the full photo.
    pub grain_cell: f32,
    /// The curves, all channels composed into one table per channel; `None` for straight curves.
    pub curves: Option<Vec<[f32; 3]>>,
}

impl Params {
    pub fn new(recipe: &Recipe) -> Self {
        let curves = (!recipe.curves.is_identity()).then(|| {
            let c = &recipe.curves;
            (0..CURVE_SAMPLES)
                .map(|i| {
                    let all = c.all.eval(i as f32 / (CURVE_SAMPLES - 1) as f32);
                    [c.red.eval(all), c.green.eval(all), c.blue.eval(all)]
                })
                .collect()
        });
        Self {
            gains: white_balance_gains(recipe.temperature, recipe.tint),
            exposure: recipe.exposure.exp2(),
            black: -recipe.blacks / 100.0 * 0.1,
            white: 1.0 - recipe.whites / 100.0 * 0.2,
            brightness: (-recipe.brightness / 100.0).exp2(),
            shadows: recipe.shadows / 100.0 * 0.25,
            highlights: recipe.highlights / 100.0 * 0.25,
            contrast: recipe.contrast / 100.0,
            saturation: recipe.saturation / 100.0,
            vibrance: recipe.vibrance / 100.0,
            vignette: recipe.vignette / 100.0,
            vignette_start: recipe.vignette_midpoint / 100.0 * 0.8,
            grain: recipe.grain / 100.0,
            grain_cell: 1.0 + recipe.grain_size / 100.0 * 3.0,
            curves,
        }
    }

    /// One pixel: `rgb` as stored (sRGB-encoded, 0–1), at `uv` (0–1 across the photo) of a photo `size` pixels large
    /// at full resolution. Returns encoded values in 0–1.
    pub fn pixel(&self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> [f32; 3] {
        let mut lin = self.light(rgb);
        if let Some(table) = &self.curves {
            for (i, c) in lin.iter_mut().enumerate() {
                *c = to_linear(lookup(table, i, to_encoded(c.max(0.0)).min(1.0)));
            }
        }
        if self.saturation != 0.0 || self.vibrance != 0.0 {
            lin = self.chroma(lin);
        }
        if self.vignette != 0.0 {
            let (u, v) = ((uv[0] - 0.5) * 2.0, (uv[1] - 0.5) * 2.0);
            let r = (u * u + v * v).sqrt() / std::f32::consts::SQRT_2;
            let t = self.vignette * smoothstep(self.vignette_start, self.vignette_start + 0.5, r);
            lin = if t < 0.0 {
                lin.map(|c| c * (1.0 + t))
            } else {
                lin.map(|c| c + t * (1.0 - c).max(0.0))
            };
        }
        let mut out = lin.map(|c| to_encoded(c.max(0.0)));
        if self.grain != 0.0 {
            let n = value_noise(uv[0] * size[0] / self.grain_cell, uv[1] * size[1] / self.grain_cell);
            out = out.map(|c| c + n * self.grain * 0.2);
        }
        out.map(|c| c.clamp(0.0, 1.0))
    }

    /// White balance, exposure and tone: the steps before the curves, on linear light.
    fn light(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut lin = rgb.map(to_linear);
        // White balance and exposure: multiplications on linear light.
        for (c, gain) in lin.iter_mut().zip(self.gains) {
            *c *= gain * self.exposure;
        }
        // Tone on the perceptual luminance, keeping the RGB ratios so hues don't shift.
        let y = luminance(lin).max(0.0);
        let toned = to_linear(self.tone(to_encoded(y)).max(0.0));
        if y > 1e-6 {
            lin.map(|c| c * toned / y)
        } else {
            [toned; 3]
        }
    }

    /// The histogram of what the curves receive from `pixels` (sRGB-encoded, 0–1) (REQ-021-18).
    pub fn histogram(&self, pixels: impl Iterator<Item = [f32; 3]>) -> Histogram {
        let mut histogram = Histogram::default();
        for rgb in pixels {
            for (c, value) in self.light(rgb).into_iter().enumerate() {
                let value = to_encoded(value.max(0.0)).min(1.0);
                let bin = ((value * HISTOGRAM_BINS as f32) as usize).min(HISTOGRAM_BINS - 1);
                histogram.channels[c][bin] += 1;
            }
        }
        histogram
    }

    /// The tone curve of blacks, whites, brightness, shadows, highlights and contrast, on a perceptual value.
    fn tone(&self, l: f32) -> f32 {
        let mut l = (l - self.black) / (self.white - self.black);
        if self.brightness != 1.0 && l > 0.0 && l < 1.0 {
            l = l.powf(self.brightness);
        }
        let t = l.clamp(0.0, 1.0);
        // Masks peaking at 1/3 and 2/3 of the range, zero at black and white.
        l += self.shadows * 6.75 * t * (1.0 - t) * (1.0 - t);
        l += self.highlights * 6.75 * (1.0 - t) * t * t;
        if self.contrast >= 0.0 {
            let x = l.clamp(0.0, 1.0);
            l + self.contrast * (x * x * (3.0 - 2.0 * x) - x)
        } else {
            l + self.contrast * 0.5 * (l - 0.5)
        }
    }

    /// Saturation and vibrance as chroma scaling in OkLab; vibrance favours muted colors and spares skin tones.
    fn chroma(&self, lin: [f32; 3]) -> [f32; 3] {
        let [l, a, b] = to_oklab(lin);
        let chroma = (a * a + b * b).sqrt();
        let mut factor = 1.0 + self.saturation;
        if self.vibrance != 0.0 {
            let muted = 1.0 - smoothstep(0.0, 0.25, chroma);
            let hue = b.atan2(a).to_degrees();
            let skin = (-((hue - 60.0) / 25.0).powi(2)).exp();
            let protect = if self.vibrance > 0.0 { 1.0 - 0.7 * skin } else { 1.0 };
            factor *= 1.0 + self.vibrance * muted * protect;
        }
        from_oklab([l, a * factor, b * factor])
    }
}

/// Bins of a [`Histogram`].
pub const HISTOGRAM_BINS: usize = 128;

/// Counts of the red, green and blue values in [`HISTOGRAM_BINS`] even steps from 0 to 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    channels: [[u32; HISTOGRAM_BINS]; 3],
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            channels: [[0; HISTOGRAM_BINS]; 3],
        }
    }
}

impl Histogram {
    /// One channel's counts; for [`Channel::All`] the three channels' together.
    pub fn get(&self, channel: Channel) -> [u32; HISTOGRAM_BINS] {
        match channel {
            Channel::All => std::array::from_fn(|i| self.channels.iter().map(|c| c[i]).sum()),
            Channel::Red => self.channels[0],
            Channel::Green => self.channels[1],
            Channel::Blue => self.channels[2],
        }
    }
}

fn lookup(table: &[[f32; 3]], channel: usize, x: f32) -> f32 {
    let at = x.clamp(0.0, 1.0) * (CURVE_SAMPLES - 1) as f32;
    let i = (at.floor() as usize).min(CURVE_SAMPLES - 2);
    let t = at - i as f32;
    table[i][channel] * (1.0 - t) + table[i + 1][channel] * t
}

/// Gains for a temperature and tint (−100 to +100): warmer is more red and less blue, more tint is less green; the
/// luminance of white stays.
pub fn white_balance_gains(temperature: f32, tint: f32) -> [f32; 3] {
    let t = temperature / 100.0 * WHITE_BALANCE_STOPS;
    let n = tint / 100.0 * WHITE_BALANCE_STOPS;
    let gains = [t.exp2(), (-n).exp2(), (-t).exp2()];
    let y = luminance(gains);
    gains.map(|g| g / y)
}

/// The temperature and tint that turn `rgb` (sRGB-encoded, as stored) neutral grey (REQ-021-05).
pub fn neutralize(rgb: [f32; 3]) -> (f32, f32) {
    let [r, g, b] = rgb.map(|c| to_linear(c).max(1e-4).log2());
    let t = (b - r) / 2.0;
    let n = g - (r + b) / 2.0;
    let scale = 100.0 / WHITE_BALANCE_STOPS;
    ((t * scale).clamp(-100.0, 100.0), (n * scale).clamp(-100.0, 100.0))
}

/// sRGB's transfer curve, extended beyond 1.
pub fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn to_encoded(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn luminance([r, g, b]: [f32; 3]) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn to_oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let [l, m, s] = [l, m, s].map(f32::cbrt);
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let [l, m, s] = [l_, m_, s_].map(|v| v * v * v);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// A hash of a lattice point, the same on the GPU (wrapping u32 arithmetic).
pub fn hash(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn lattice(x: i32, y: i32) -> f32 {
    (hash(x as u32, y as u32) >> 8) as f32 / 16_777_216.0 - 0.5
}

/// Smooth value noise in −0.5…0.5, one lattice cell per unit.
pub fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (ix as i32, iy as i32);
    let top = lattice(ix, iy) * (1.0 - sx) + lattice(ix + 1, iy) * sx;
    let bottom = lattice(ix, iy + 1) * (1.0 - sx) + lattice(ix + 1, iy + 1) * sx;
    top * (1.0 - sy) + bottom * sy
}

/// Triangular dither for quantizing to `levels` steps at pixel `(x, y)`: −1…+1 steps, the same each time.
pub fn dither(x: u32, y: u32, channel: u32) -> f32 {
    let a = (hash(x, y.wrapping_mul(3).wrapping_add(channel)) >> 8) as f32 / 16_777_216.0;
    let b = (hash(y.wrapping_add(0x9e37), x.wrapping_mul(7).wrapping_add(channel)) >> 8) as f32 / 16_777_216.0;
    a - b
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: [f32; 2] = [100.0, 100.0];
    const MID: [f32; 2] = [0.5, 0.5];

    fn render(recipe: &Recipe, rgb: [f32; 3]) -> [f32; 3] {
        Params::new(recipe).pixel(rgb, MID, SIZE)
    }

    fn with(adjustment: Adjustment, value: f32) -> Recipe {
        let mut recipe = Recipe::default();
        adjustment.set(&mut recipe, value);
        recipe
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
    }

    fn grey(v: f32) -> [f32; 3] {
        [v; 3]
    }

    fn samples() -> Vec<[f32; 3]> {
        let mut colors = vec![
            grey(0.0),
            grey(1.0),
            grey(0.5),
            [0.9, 0.2, 0.1],
            [0.1, 0.6, 0.9],
            [0.8, 0.6, 0.45],
        ];
        colors.extend((0..=10).map(|i| grey(i as f32 / 10.0)));
        colors
    }

    #[test]
    fn req_021_03_req_021_05_req_021_06_each_adjustment_at_its_neutral_value_changes_nothing() {
        let neutral = Recipe::default();
        assert!(neutral.is_neutral());
        for rgb in samples() {
            for uv in [[0.0, 0.0], MID, [1.0, 0.3]] {
                let out = Params::new(&neutral).pixel(rgb, uv, SIZE);
                assert!(close(out, rgb), "{rgb:?} → {out:?}");
            }
        }
        for adjustment in Adjustment::ALL.into_iter().filter(|a| a.changes_pixels()) {
            assert!(
                !with(adjustment, 50.0f32.min(adjustment.range().1)).is_neutral(),
                "{adjustment:?}"
            );
        }
        // The shape of an effect alone does nothing.
        assert!(with(Adjustment::GrainSize, 80.0).is_neutral());
        assert!(with(Adjustment::VignetteMidpoint, 10.0).is_neutral());
    }

    #[test]
    fn req_021_03_exposure_scales_linear_light_by_two_to_the_ev() {
        let rgb = grey(0.4);
        let out = render(&with(Adjustment::Exposure, 1.0), rgb);
        assert!((to_linear(out[0]) - 2.0 * to_linear(0.4)).abs() < 1e-4);
        let out = render(&with(Adjustment::Exposure, -2.0), rgb);
        assert!((to_linear(out[0]) - to_linear(0.4) / 4.0).abs() < 1e-4);
        // Hue stays: a color keeps its channel ratios.
        let out = render(&with(Adjustment::Exposure, -1.0), [0.8, 0.4, 0.2]);
        let ratio = to_linear(out[0]) / to_linear(out[1]);
        assert!((ratio - to_linear(0.8) / to_linear(0.4)).abs() < 1e-3);
    }

    #[test]
    fn req_021_03_contrast_steepens_or_flattens_around_the_midtone() {
        let more = with(Adjustment::Contrast, 100.0);
        let less = with(Adjustment::Contrast, -100.0);
        assert!(render(&more, grey(0.25))[0] < 0.25 && render(&more, grey(0.75))[0] > 0.75);
        assert!(render(&less, grey(0.25))[0] > 0.25 && render(&less, grey(0.75))[0] < 0.75);
        assert!((render(&more, grey(0.5))[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn req_021_03_shadows_and_highlights_move_only_their_tones() {
        let shadows = with(Adjustment::Shadows, 100.0);
        assert!(render(&shadows, grey(0.25))[0] > 0.3);
        assert!((render(&shadows, grey(0.95))[0] - 0.95).abs() < 0.01);
        assert_eq!(render(&shadows, grey(0.0))[0], 0.0);
        let highlights = with(Adjustment::Highlights, -100.0);
        assert!(render(&highlights, grey(0.75))[0] < 0.7);
        assert!((render(&highlights, grey(0.05))[0] - 0.05).abs() < 0.01);
        assert!((render(&highlights, grey(1.0))[0] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn req_021_03_whites_and_blacks_move_the_white_and_black_points() {
        assert!(render(&with(Adjustment::Blacks, 100.0), grey(0.0))[0] > 0.05);
        assert!(render(&with(Adjustment::Blacks, -100.0), grey(0.08))[0] < 0.01);
        assert!(render(&with(Adjustment::Whites, 100.0), grey(0.85))[0] >= 0.999);
        assert!(render(&with(Adjustment::Whites, -100.0), grey(1.0))[0] < 0.9);
    }

    #[test]
    fn req_021_17_brightness_lifts_or_lowers_the_midtones_only() {
        let brighter = with(Adjustment::Brightness, 100.0);
        let darker = with(Adjustment::Brightness, -100.0);
        assert!(render(&brighter, grey(0.5))[0] > 0.6);
        assert!(render(&darker, grey(0.5))[0] < 0.4);
        for recipe in [&brighter, &darker] {
            assert!(render(recipe, grey(0.0))[0] < 1e-4);
            assert!((render(recipe, grey(1.0))[0] - 1.0).abs() < 1e-4);
        }
        // Old recipes without it read as 0.
        let old = Recipe::from_json(r#"{"version":1,"exposure":0.5}"#).unwrap();
        assert_eq!(old.brightness, 0.0);
    }

    #[test]
    fn req_021_18_the_histogram_shows_the_tones_the_curve_receives() {
        let pixels = [grey(0.25), grey(0.25), [1.0, 0.5, 0.0]];
        let bin = |v: f32| ((v * HISTOGRAM_BINS as f32) as usize).min(HISTOGRAM_BINS - 1);
        let histogram = Params::new(&Recipe::default()).histogram(pixels.iter().copied());
        assert_eq!(histogram.get(Channel::Red)[bin(0.25)], 2);
        assert_eq!(histogram.get(Channel::Red)[bin(1.0)], 1);
        assert_eq!(histogram.get(Channel::Green)[bin(0.5)], 1);
        assert_eq!(histogram.get(Channel::Blue)[bin(0.0)], 1);
        // All channels together.
        assert_eq!(histogram.get(Channel::All).iter().sum::<u32>(), 9);
        assert_eq!(histogram.get(Channel::All)[bin(0.25)], 6);

        // Adjustments before the curves move it, the curves don't.
        let brighter = Params::new(&with(Adjustment::Exposure, 1.0)).histogram(pixels.iter().copied());
        assert_eq!(brighter.get(Channel::Red)[bin(0.25)], 0);
        let mut curved = Recipe::default();
        curved.curves.all.add_point(0.25, 0.6);
        assert_eq!(Params::new(&curved).histogram(pixels.iter().copied()), histogram);
    }

    #[test]
    fn req_021_04_a_curve_maps_tones_through_its_points_without_overshooting() {
        let mut curve = Curve::default();
        assert!(curve.is_identity());
        let i = curve.add_point(0.25, 0.4).unwrap();
        assert_eq!(i, 1);
        curve.add_point(0.75, 0.8).unwrap();
        assert!((curve.eval(0.25) - 0.4).abs() < 1e-5 && (curve.eval(0.75) - 0.8).abs() < 1e-5);
        // Monotone between monotone points.
        let values: Vec<f32> = (0..=100).map(|i| curve.eval(i as f32 / 100.0)).collect();
        assert!(values.windows(2).all(|w| w[1] >= w[0] - 1e-6));
        // A flat stretch stays flat: no overshoot.
        let flat = Curve {
            points: vec![[0.0, 0.0], [0.3, 0.5], [0.7, 0.5], [1.0, 1.0]],
        };
        assert!((0..=40).all(|i| (flat.eval(0.3 + i as f32 / 100.0) - 0.5).abs() < 1e-5));
        // Through the recipe: all channels, then one.
        let mut recipe = Recipe::default();
        recipe.curves.all = curve.clone();
        let out = render(&recipe, grey(0.25));
        assert!(out.iter().all(|c| (c - 0.4).abs() < 0.01), "{out:?}");
        let mut recipe = Recipe::default();
        recipe.curves.red = curve;
        let out = render(&recipe, grey(0.25));
        assert!((out[0] - 0.4).abs() < 0.01 && (out[1] - 0.25).abs() < 0.01 && (out[2] - 0.25).abs() < 0.01);
    }

    #[test]
    fn req_021_04_curve_points_are_added_moved_and_removed_within_bounds() {
        let mut curve = Curve::default();
        curve.add_point(0.5, 0.6);
        assert_eq!(curve.add_point(0.505, 0.1), None, "too close to another point");
        curve.move_point(1, 2.0, -1.0);
        assert!(curve.points[1][0] < 1.0 && curve.points[1][1] == 0.0);
        assert!(!curve.remove_point(0) && !curve.remove_point(2));
        assert!(curve.remove_point(1));
        assert!(curve.is_identity());
        for i in 0..40 {
            curve.add_point(i as f32 / 40.0 + 0.01, 0.5);
        }
        assert_eq!(curve.points.len(), MAX_CURVE_POINTS);
    }

    #[test]
    fn req_021_05_temperature_and_tint_shift_colors_along_their_axes() {
        let warm = render(&with(Adjustment::Temperature, 60.0), grey(0.5));
        assert!(warm[0] > 0.5 && warm[2] < 0.5);
        let cool = render(&with(Adjustment::Temperature, -60.0), grey(0.5));
        assert!(cool[0] < 0.5 && cool[2] > 0.5);
        let magenta = render(&with(Adjustment::Tint, 60.0), grey(0.5));
        assert!(magenta[1] < magenta[0] && magenta[1] < magenta[2]);
        let green = render(&with(Adjustment::Tint, -60.0), grey(0.5));
        assert!(green[1] > green[0] && green[1] > green[2]);
    }

    #[test]
    fn req_021_05_the_picker_makes_the_picked_spot_neutral() {
        for picked in [[0.6, 0.5, 0.4], [0.4, 0.5, 0.62], [0.5, 0.58, 0.5]] {
            let (temperature, tint) = neutralize(picked);
            let recipe = Recipe {
                temperature,
                tint,
                ..Default::default()
            };
            let out = render(&recipe, picked);
            assert!(
                (out[0] - out[1]).abs() < 2e-3 && (out[1] - out[2]).abs() < 2e-3,
                "{picked:?} → {out:?}"
            );
        }
    }

    #[test]
    fn req_021_05_saturation_scales_all_colors_and_vibrance_mostly_the_muted() {
        let chroma = |rgb: [f32; 3]| {
            let [_, a, b] = to_oklab(rgb.map(to_linear));
            (a * a + b * b).sqrt()
        };
        let none = render(&with(Adjustment::Saturation, -100.0), [0.8, 0.3, 0.2]);
        assert!(chroma(none) < 1e-3, "{none:?}");
        let (muted, vivid) = ([0.55, 0.5, 0.45], [0.9, 0.1, 0.1]);
        let gain = |recipe: &Recipe, rgb| chroma(render(recipe, rgb)) / chroma(rgb);
        let vibrance = with(Adjustment::Vibrance, 100.0);
        assert!(gain(&vibrance, muted) > gain(&vibrance, vivid) + 0.3);
        // A skin tone gains less than a muted blue of similar chroma.
        let (skin, blue) = ([0.85, 0.65, 0.52], [0.52, 0.62, 0.8]);
        assert!(gain(&vibrance, skin) < gain(&vibrance, blue));
        // Greys stay grey.
        let out = render(&with(Adjustment::Saturation, 100.0), grey(0.5));
        assert!(close(out, grey(0.5)));
    }

    #[test]
    fn req_021_06_vignette_darkens_or_lightens_the_edges_only() {
        let dark = Params::new(&with(Adjustment::Vignette, -100.0));
        assert!(close(dark.pixel(grey(0.5), MID, SIZE), grey(0.5)));
        assert!(dark.pixel(grey(0.5), [0.0, 0.0], SIZE)[0] < 0.1);
        let light = Params::new(&with(Adjustment::Vignette, 100.0));
        assert!(light.pixel(grey(0.5), [1.0, 1.0], SIZE)[0] > 0.9);
    }

    #[test]
    fn req_021_06_grain_is_the_same_for_the_same_position_and_follows_the_photo_not_the_screen() {
        let params = Params::new(&with(Adjustment::Grain, 100.0));
        let a = params.pixel(grey(0.5), [0.31, 0.62], [4000.0, 3000.0]);
        assert_eq!(a, params.pixel(grey(0.5), [0.31, 0.62], [4000.0, 3000.0]));
        // Noise varies across the photo, around the original value.
        let values: Vec<f32> = (0..200)
            .map(|i| params.pixel(grey(0.5), [i as f32 / 200.0, 0.5], [4000.0, 3000.0])[0])
            .collect();
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        assert!((mean - 0.5).abs() < 0.03, "{mean}");
        assert!(values.iter().any(|v| (v - 0.5).abs() > 0.02));
        // Grain is monochrome.
        assert!(a[0] == a[1] && a[1] == a[2]);
        assert!((-0.5..=0.5).contains(&value_noise(12.3, -4.5)));
    }

    #[test]
    fn req_021_07_history_undoes_and_redoes_changes() {
        let mut history = History::new(Recipe::default());
        assert!(!history.can_undo() && !history.can_redo());
        history.push(with(Adjustment::Exposure, 1.0));
        history.push(with(Adjustment::Exposure, 1.0));
        history.push(with(Adjustment::Exposure, 2.0));
        assert!(history.undo());
        assert_eq!(history.current().exposure, 1.0);
        assert!(history.undo());
        assert!(history.current().is_neutral());
        assert!(!history.undo());
        assert!(history.redo());
        // A new change drops what could have been redone.
        history.push(with(Adjustment::Contrast, 10.0));
        assert!(!history.can_redo());
        assert_eq!(history.current().contrast, 10.0);
    }

    #[test]
    fn req_021_09_copies_are_named_after_the_original_and_numbered() {
        assert_eq!(copy_name("IMG_1.jpg", |_| false), "IMG_1 (edited).jpg");
        let taken = ["IMG_1 (edited).jpg", "IMG_1 (edited 2).jpg"];
        assert_eq!(
            copy_name("IMG_1.jpg", |name| taken.contains(&name)),
            "IMG_1 (edited 3).jpg"
        );
        assert_eq!(copy_name("noext", |_| false), "noext (edited)");
        assert_eq!(copy_name("a.b.PNG", |_| false), "a.b (edited).PNG");
    }

    #[test]
    fn req_021_10_backups_go_into_the_housekeeping_folder_of_the_library() {
        let backup = backup_rel_path("2024/trip/a.jpg");
        assert_eq!(backup, ".good-gallery/originals/2024/trip/a.jpg");
        assert!(crate::media::is_ignored_dir(backup.split('/').next().unwrap()));
        assert!(crate::media::is_safe_rel_path(&backup));
    }

    #[test]
    fn req_021_01_jpeg_png_and_webp_are_editable() {
        for name in ["a.jpg", "b.JPEG", "c.png", "d.webp"] {
            assert!(is_editable(name), "{name}");
        }
        for name in ["a.gif", "b.avif", "c.mp4", ".jpg", "jpg"] {
            assert!(!is_editable(name), "{name}");
        }
    }

    #[test]
    fn req_021_11_recipes_round_trip_through_json_and_hostile_ones_are_tamed() {
        let mut recipe = with(Adjustment::Exposure, 1.5);
        recipe.curves.blue.add_point(0.5, 0.7);
        assert_eq!(Recipe::from_json(&recipe.to_json()), Some(recipe));
        assert_eq!(Recipe::from_json("not json"), None);
        assert_eq!(Recipe::from_json(r#"{"version": 99}"#), None);
        // Missing fields are neutral; values out of range are clamped, curves sorted and capped.
        let tamed = Recipe::from_json(
            r#"{"exposure": 1e9, "contrast": -400, "curves": {"all": [[1, 1], [0.5, 9], [0, 0]], "red": []}}"#,
        )
        .unwrap();
        assert_eq!(tamed.exposure, 5.0);
        assert_eq!(tamed.contrast, -100.0);
        assert_eq!(tamed.curves.all.points, vec![[0.0, 0.0], [0.5, 1.0], [1.0, 1.0]]);
        assert!(tamed.curves.red.is_identity());
        let many: Vec<String> = (0..1000).map(|i| format!("[{}, 0.5]", i as f32 / 1000.0)).collect();
        let json = format!(r#"{{"curves": {{"green": [{}]}}}}"#, many.join(","));
        assert!(Recipe::from_json(&json).unwrap().curves.green.points.len() <= MAX_CURVE_POINTS);
    }

    #[test]
    fn req_021_11_a_saved_edit_needs_a_readable_recipe_and_a_source_inside_the_library() {
        let recipe = with(Adjustment::Contrast, 20.0).to_json();
        let edit = SavedEdit::parse(Some(&recipe), Some(".good-gallery/originals/a/b.jpg")).unwrap();
        assert_eq!(edit.recipe().contrast, 20.0);
        assert!(edit.replaced_original("a/b.jpg"));
        assert!(!edit.replaced_original("a/b (edited).jpg"));
        assert_eq!(SavedEdit::parse(Some(&recipe), None), None);
        assert_eq!(SavedEdit::parse(Some("{]"), Some("a.jpg")), None);
        assert_eq!(SavedEdit::parse(Some(&recipe), Some("../outside.jpg")), None);
    }

    #[test]
    fn req_021_13_dither_stays_within_one_step_and_repeats() {
        let values: Vec<f32> = (0..1000).map(|i| dither(i, i * 7, 0)).collect();
        assert!(values.iter().all(|v| (-1.0..=1.0).contains(v)));
        assert!(values.iter().sum::<f32>().abs() / 1000.0 < 0.05);
        assert_eq!(dither(5, 9, 2), dither(5, 9, 2));
    }

    #[test]
    fn req_021_15_the_date_modified_setting_parses_its_values() {
        assert_eq!(MtimeOnSave::default(), MtimeOnSave::Keep);
        for value in [MtimeOnSave::Keep, MtimeOnSave::Update] {
            assert_eq!(MtimeOnSave::parse(value.as_str()), Some(value));
        }
        assert_eq!(MtimeOnSave::parse("sometimes"), None);
    }
}
