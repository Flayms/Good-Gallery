//! Photo editing (spec 021): an edit is a [`Recipe`] of adjustments, rendered per pixel by [`Params::pixel`] from the
//! unedited original. The viewer's preview runs the same formulas on the GPU (`app::edit`'s shader mirrors this file);
//! saving runs them here. Also the undo [`History`], the name of a saved copy and where backups of originals go.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The recipe format. Recipes of a newer version than this are not read (a newer app wrote them). A recipe is written
/// with the oldest version that describes it: 2 only with flips or straightening (REQ-029-10, REQ-029-11), which an
/// app that knows 1 would leave out.
pub const RECIPE_VERSION: u32 = 2;

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
    /// Its versions as stored (`gg:EditHistory`, [`crate::versions`]); empty for none (#247).
    pub history: String,
}

impl SavedEdit {
    /// From a file's tags: both present, a recipe this version reads, and a source inside the library (REQ-011-08).
    pub fn parse(recipe: Option<&str>, source: Option<&str>) -> Option<Self> {
        let (recipe, source) = (recipe?, source?);
        (Recipe::from_json(recipe).is_some() && crate::media::is_safe_rel_path(source)).then(|| Self {
            recipe: recipe.to_owned(),
            source: source.to_owned(),
            history: String::new(),
        })
    }

    /// With the file's `gg:EditHistory`, if it has one.
    pub fn with_history(mut self, history: Option<&str>) -> Self {
        self.history = history.unwrap_or_default().to_owned();
        self
    }

    /// The versions the file was saved with, ending with its edit (REQ-021-24).
    pub fn versions(&self) -> Vec<crate::versions::Version> {
        crate::versions::of_file(&self.recipe(), &self.history)
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
    /// Degrees around the color wheel, −180 to +180.
    pub hue: f32,
    pub vibrance: f32,
    pub saturation: f32,
    /// 0 to 100.
    pub grain: f32,
    /// 0 to 100.
    pub grain_size: f32,
    pub vignette: f32,
    /// 0 to 100.
    pub vignette_midpoint: f32,
    /// How the photo is blurred (REQ-023-01); a kind only a newer version knows reads as none.
    #[serde(deserialize_with = "blur_kind_or_none")]
    pub blur_kind: BlurKind,
    /// 0 to 100.
    pub blur: f32,
    /// The sharp area's size, 0 to 100.
    pub blur_size: f32,
    /// How far the blur takes to grow beyond it, 0 to 100.
    pub blur_feather: f32,
    /// Where the sharp area is, 0–1 across the photo.
    pub blur_focus: [f32; 2],
    /// _Lens_ instead of _Soft_: lights spread into discs (REQ-023-05).
    pub blur_lens: bool,
    /// The subject's own light and color, on top of the rest (REQ-022-03); `None` while neutral.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<Box<Recipe>>,
    /// The background's own light and color.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Box<Recipe>>,
    /// What brush and freeform masks painted; their shapes name these by place (REQ-030-12, REQ-030-13).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paintings: Vec<crate::paint::Painting>,
    /// Masks with their own light and color, on top of the rest, in order (REQ-030-02).
    #[serde(skip_serializing_if = "Vec::is_empty", deserialize_with = "masks_known")]
    pub masks: Vec<LocalMask>,
    /// A film look under the adjustments (REQ-028-01); one only a newer version knows reads as none.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "look_or_none")]
    pub look: Option<crate::look::LookChoice>,
    /// Quarter turns clockwise, 0 to 3 (REQ-029-01).
    #[serde(skip_serializing_if = "is_zero")]
    pub quarter_turns: u8,
    /// `x, y, w, h` as fractions of the upright, unturned original, so it stays on the content (REQ-029-02); `None`
    /// for the whole photo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f32; 4]>,
    /// Degrees clockwise, within ±[`MAX_STRAIGHTEN`] (REQ-029-10); the crop lies on the straightened canvas.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub straighten: f32,
    /// The upright original's width per height while straightened, which the canvas needs (REQ-029-10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect: Option<f32>,
    /// Mirrored left to right and top to bottom, as the original, before straightening (REQ-029-11).
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_h: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_v: bool,
    /// The saved photo's long edge in pixels, after the crop and turns (REQ-029-09); `None` for its own size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_edge: Option<u32>,
    /// JPEG and WebP quality, 10 to 100 (REQ-021-21); `None` for [`DEFAULT_QUALITY`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
}

/// The quality saved JPEGs and WebPs get unless the edit sets one (REQ-021-21).
pub const DEFAULT_QUALITY: u8 = 95;
/// The lowest quality and the shortest long edge an edit can ask for.
pub const MIN_QUALITY: u8 = 10;
pub const MIN_LONG_EDGE: u32 = 16;

/// The size of a `width` × `height` photo with its long edge at most `long_edge`, aspect ratio kept and never
/// enlarged (REQ-029-09).
pub fn scaled_size(width: u32, height: u32, long_edge: Option<u32>) -> (u32, u32) {
    let long = width.max(height);
    let Some(edge) = long_edge.filter(|&edge| edge < long) else {
        return (width, height);
    };
    let scale = |side: u32| ((f64::from(side) * f64::from(edge) / f64::from(long)).round() as u32).max(1);
    (scale(width), scale(height))
}

fn is_zero(turns: &u8) -> bool {
    *turns == 0
}

fn is_zero_f32(value: &f32) -> bool {
    *value == 0.0
}

fn is_false(value: &bool) -> bool {
    !*value
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
            hue: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            grain: 0.0,
            grain_size: 25.0,
            vignette: 0.0,
            vignette_midpoint: 50.0,
            blur_kind: BlurKind::None,
            blur: 0.0,
            blur_size: 30.0,
            blur_feather: 50.0,
            blur_focus: [0.5, 0.5],
            blur_lens: false,
            subject: None,
            background: None,
            paintings: Vec::new(),
            masks: Vec::new(),
            look: None,
            quarter_turns: 0,
            crop: None,
            straighten: 0.0,
            aspect: None,
            flip_h: false,
            flip_v: false,
            long_edge: None,
            quality: None,
        }
    }
}

impl Recipe {
    /// Whether it leaves every pixel as it is.
    pub fn is_neutral(&self) -> bool {
        Adjustment::ALL
            .iter()
            .filter(|adjustment| adjustment.changes_pixels())
            // A blur strength without a kind of blur does nothing.
            .filter(|adjustment| **adjustment != Adjustment::Blur || self.blur_kind != BlurKind::None)
            .all(|adjustment| adjustment.get(self) == adjustment.neutral())
            && self.curves.is_identity()
            && self.look.is_none()
            && self.geometry().is_identity()
            && self.long_edge.is_none()
            && self.quality.is_none()
            && [&self.subject, &self.background]
                .into_iter()
                .all(|part| part.as_ref().is_none_or(|part| part.is_neutral()))
            && self.masks.iter().all(|mask| mask.recipe.is_neutral())
    }

    /// The adjustments of `part`: the recipe itself for [`Part::Photo`], `None` for a part without any.
    pub fn part(&self, part: Part) -> Option<&Recipe> {
        match part {
            Part::Photo => Some(self),
            Part::Subject => self.subject.as_deref(),
            Part::Background => self.background.as_deref(),
            Part::Mask(i) => self.masks.get(i).map(|mask| &*mask.recipe),
        }
    }

    /// The adjustments of `part`, made neutral if it had none yet; [`Self::tidy`] drops them again while neutral.
    pub fn part_mut(&mut self, part: Part) -> &mut Recipe {
        match part {
            Part::Photo => self,
            Part::Subject => self.subject.get_or_insert_default(),
            Part::Background => self.background.get_or_insert_default(),
            Part::Mask(i) if i < self.masks.len() => &mut self.masks[i].recipe,
            // No such mask (any more): nothing that is shown changes.
            Part::Mask(_) => self,
        }
    }

    /// Its crop and quarter turns.
    pub fn geometry(&self) -> Geometry {
        Geometry::new(self.crop, self.quarter_turns)
            .with_straighten(self.straighten, self.aspect.unwrap_or(1.0))
            .with_flips(self.flip_h, self.flip_v)
    }

    /// Straightens by `degrees` clockwise a photo `aspect` wide per unit of height (REQ-029-10). The crop becomes the
    /// largest of its shape, centered, that shows only the photo.
    pub fn set_straighten(&mut self, degrees: f32, aspect: f32) {
        let ratio = self.geometry().with_straighten(self.straighten, aspect).crop_ratio();
        self.straighten = sanitized_straighten(degrees);
        self.aspect = (self.straighten != 0.0).then(|| sanitized_aspect(aspect));
        let canvas = self.geometry().with_straighten(self.straighten, aspect);
        self.crop = sanitized_crop(canvas.largest_crop(ratio));
    }

    /// Mirrors the output left to right (`horizontal`) or top to bottom as it shows on screen, crop and straightening
    /// included (REQ-029-11).
    pub fn flip(&mut self, horizontal: bool) {
        // A quarter turn swaps which side of the canvas is which on screen.
        let across = horizontal != (self.quarter_turns % 2 == 1);
        let [x, y, w, h] = self.geometry().crop();
        if across {
            self.flip_h = !self.flip_h;
            self.crop = sanitized_crop([1.0 - x - w, y, w, h]);
        } else {
            self.flip_v = !self.flip_v;
            self.crop = sanitized_crop([x, 1.0 - y - h, w, h]);
        }
        self.straighten = -self.straighten + 0.0;
    }

    /// Whether rendering needs the photo's subject: for the parts' adjustments or a background blur.
    pub fn needs_subject(&self) -> bool {
        self.has_parts() || (self.blur_kind == BlurKind::Background && self.blur > 0.0)
    }

    /// Whether the subject or the background has adjustments of its own.
    pub fn has_parts(&self) -> bool {
        self.subject.is_some() || self.background.is_some()
    }

    /// Drops the parts that are neutral, and from the parts everything but light and color.
    pub fn tidy(&mut self) {
        let light_and_color = |recipe: &Recipe| {
            let mut kept = Recipe::default();
            for adjustment in Adjustment::ALL.into_iter().filter(|a| a.in_parts()) {
                adjustment.set(&mut kept, adjustment.get(recipe));
            }
            kept
        };
        for part in [&mut self.subject, &mut self.background] {
            if let Some(recipe) = part {
                **recipe = light_and_color(recipe);
            }
            if part.as_ref().is_some_and(|recipe| recipe.is_neutral()) {
                *part = None;
            }
        }
        // A neutral mask stays: it was just added, or is about to get its adjustments.
        for mask in &mut self.masks {
            *mask.recipe = light_and_color(&mask.recipe);
        }
        self.tidy_paintings();
        // The defaults are no change (REQ-021-21, REQ-029-09).
        self.quality = self
            .quality
            .map(|quality| quality.clamp(MIN_QUALITY, 100))
            .filter(|&quality| quality != DEFAULT_QUALITY);
        self.long_edge = self.long_edge.map(|edge| edge.max(MIN_LONG_EDGE));
    }

    /// The quality to save at (REQ-021-21).
    pub fn quality(&self) -> u8 {
        self.quality.unwrap_or(DEFAULT_QUALITY)
    }

    /// Drops the paintings no mask uses any more, and has the masks name the rest by their new places.
    fn tidy_paintings(&mut self) {
        let used: Vec<usize> = (0..self.paintings.len())
            .filter(|&layer| {
                self.masks
                    .iter()
                    .any(|mask| matches!(mask.shape, MaskShape::Painted { layer: at, .. } if usize::from(at) == layer))
            })
            .collect();
        if used.len() == self.paintings.len() {
            return;
        }
        for mask in &mut self.masks {
            if let MaskShape::Painted { layer, .. } = &mut mask.shape
                && let Some(place) = used.iter().position(|&at| at == usize::from(*layer))
            {
                *layer = place as u8;
            }
        }
        let mut layer = 0;
        self.paintings.retain(|_| {
            layer += 1;
            used.contains(&(layer - 1))
        });
    }

    /// Compact JSON, as stored in a saved file (`gg:EditRecipe`) and the index.
    pub fn to_json(&self) -> String {
        let mut recipe = self.clone();
        recipe.version = if self.geometry().transforms() { 2 } else { 1 };
        serde_json::to_string(&recipe).unwrap_or_default()
    }

    /// A recipe from a file or the index, with every value brought into its range (REQ-011-10). `None` for invalid
    /// JSON and for recipes of a newer version.
    pub fn from_json(json: &str) -> Option<Self> {
        let mut recipe: Self = serde_json::from_str(json).ok()?;
        if recipe.version > RECIPE_VERSION {
            return None;
        }
        recipe.sanitize();
        for part in [&mut recipe.subject, &mut recipe.background].into_iter().flatten() {
            part.sanitize();
        }
        recipe.masks.truncate(MAX_MASKS);
        crate::paint::sanitize(&mut recipe.paintings);
        // A painted mask without its painting selects nothing it could ever show: it goes.
        let paintings = recipe.paintings.len();
        recipe
            .masks
            .retain(|mask| !matches!(mask.shape, MaskShape::Painted { layer, .. } if usize::from(layer) >= paintings));
        for mask in &mut recipe.masks {
            mask.recipe.sanitize();
            mask.shape.sanitize();
        }
        recipe.tidy();
        Some(recipe)
    }

    fn sanitize(&mut self) {
        self.version = RECIPE_VERSION;
        for adjustment in Adjustment::ALL {
            let value = adjustment.get(self);
            let (low, high) = adjustment.range();
            let value = if value.is_finite() {
                value.clamp(low, high)
            } else {
                adjustment.neutral()
            };
            adjustment.set(self, value);
        }
        for channel in Channel::ALL {
            self.curves.get_mut(channel).sanitize();
        }
        self.blur_focus = self
            .blur_focus
            .map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.5 });
        if let Some(look) = &mut self.look {
            look.sanitize();
        }
        self.quarter_turns %= 4;
        self.crop = self.crop.and_then(sanitized_crop);
        self.straighten = sanitized_straighten(self.straighten);
        self.aspect = (self.straighten != 0.0).then(|| sanitized_aspect(self.aspect.unwrap_or(1.0)));
    }
}

/// A crop inside the photo; `None` for an invalid one and for the whole photo (REQ-011-10).
fn sanitized_crop([x, y, w, h]: [f32; 4]) -> Option<[f32; 4]> {
    if ![x, y, w, h].iter().all(|n| n.is_finite()) || !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) {
        return None;
    }
    let (w, h) = (w.min(1.0 - x), h.min(1.0 - y));
    let whole = x == 0.0 && y == 0.0 && w >= 1.0 && h >= 1.0;
    (w > 0.0 && h > 0.0 && !whole).then_some([x, y, w, h])
}

/// The steepest straightening either way, in degrees (REQ-029-10).
pub const MAX_STRAIGHTEN: f32 = 100.0;

/// A recipe's flips, straightening, crop and quarter turns: how the rendered photo becomes the output (spec 029). The
/// upright original is flipped, then straightened about its center onto a canvas the size of the turned photo's
/// bounding box, then cropped on that canvas, then turned by quarter turns. Without flips and straightening the canvas
/// is the original itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    crop: [f32; 4],
    quarter_turns: u8,
    /// Degrees clockwise, within ±[`MAX_STRAIGHTEN`].
    straighten: f32,
    /// Mirrored left to right, top to bottom, before straightening.
    flip: [bool; 2],
    /// The upright original's width over its height; only straightening needs it.
    aspect: f32,
}

impl Geometry {
    pub fn new(crop: Option<[f32; 4]>, quarter_turns: u8) -> Self {
        Self {
            crop: crop.and_then(sanitized_crop).unwrap_or([0.0, 0.0, 1.0, 1.0]),
            quarter_turns: quarter_turns % 4,
            straighten: 0.0,
            flip: [false; 2],
            aspect: 1.0,
        }
    }

    /// Straightened by `degrees` clockwise, for an original `aspect` wide per unit of height (REQ-029-10).
    pub fn with_straighten(mut self, degrees: f32, aspect: f32) -> Self {
        self.straighten = sanitized_straighten(degrees);
        self.aspect = sanitized_aspect(aspect);
        self
    }

    /// Mirrored as the original, before straightening (REQ-029-11).
    pub fn with_flips(mut self, horizontal: bool, vertical: bool) -> Self {
        self.flip = [horizontal, vertical];
        self
    }

    /// The same, without its crop: the whole canvas.
    pub fn uncropped(mut self) -> Self {
        self.crop = [0.0, 0.0, 1.0, 1.0];
        self
    }

    pub fn is_identity(&self) -> bool {
        self.quarter_turns == 0 && self.crop == [0.0, 0.0, 1.0, 1.0] && !self.transforms()
    }

    /// Whether flips or straightening move the pixels about, beyond a crop and quarter turns.
    pub fn transforms(&self) -> bool {
        self.straighten != 0.0 || self.flip.contains(&true)
    }

    /// The crop as `x, y, w, h`, fractions of the canvas (the upright, unturned original without straightening); the
    /// whole canvas without one.
    pub fn crop(&self) -> [f32; 4] {
        self.crop
    }

    pub fn quarter_turns(&self) -> u8 {
        self.quarter_turns
    }

    pub fn straighten(&self) -> f32 {
        self.straighten
    }

    pub fn flips(&self) -> [bool; 2] {
        self.flip
    }

    pub fn aspect(&self) -> f32 {
        self.aspect
    }

    /// The cosine and sine of the straightening.
    fn turn(&self) -> (f32, f32) {
        let radians = self.straighten.to_radians();
        (radians.cos(), radians.sin())
    }

    /// The canvas's size per the original's: its width per the original's width, its height per its height.
    pub fn canvas_scale(&self) -> [f32; 2] {
        if self.straighten == 0.0 {
            return [1.0, 1.0];
        }
        let (c, s) = self.turn();
        let (c, s) = (c.abs(), s.abs());
        [c + s / self.aspect, s * self.aspect + c]
    }

    /// The canvas's size in pixels for an upright original of this size.
    pub fn canvas_size(&self, width: u32, height: u32) -> (u32, u32) {
        let [sx, sy] = self.canvas_scale();
        (
            ((width as f32 * sx).round() as u32).max(1),
            ((height as f32 * sy).round() as u32).max(1),
        )
    }

    /// Where a point of the original (fractions of its size) lies on the canvas.
    pub fn to_canvas(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let u = if self.flip[0] { 1.0 - u } else { u };
        let v = if self.flip[1] { 1.0 - v } else { v };
        if self.straighten == 0.0 {
            return [u, v];
        }
        let (c, s) = self.turn();
        let [sx, sy] = self.canvas_scale();
        // In units of the original's height, about its center.
        let (x, y) = ((u - 0.5) * self.aspect, v - 0.5);
        let (x, y) = (x * c - y * s, x * s + y * c);
        [x / (self.aspect * sx) + 0.5, y / sy + 0.5]
    }

    /// Where a point of the canvas lies on the original; outside 0–1 where the canvas shows none of it.
    pub fn from_canvas(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let [u, v] = if self.straighten == 0.0 {
            [u, v]
        } else {
            let (c, s) = self.turn();
            let [sx, sy] = self.canvas_scale();
            let (x, y) = ((u - 0.5) * self.aspect * sx, (v - 0.5) * sy);
            let (x, y) = (x * c + y * s, -x * s + y * c);
            [x / self.aspect + 0.5, y + 0.5]
        };
        [
            if self.flip[0] { 1.0 - u } else { u },
            if self.flip[1] { 1.0 - v } else { v },
        ]
    }

    /// Whether a point of the canvas shows the photo, not the empty corners around a straightened one.
    pub fn shows_photo(&self, point: [f32; 2]) -> bool {
        const SLACK: f32 = 1e-4;
        self.from_canvas(point)
            .iter()
            .all(|side| (-SLACK..=1.0 + SLACK).contains(side))
    }

    /// Whether a point of the original (fractions of its size, as [`Self::to_original`] gives it) is on the photo.
    pub fn shows_photo_at(&self, point: [f32; 2]) -> bool {
        const SLACK: f32 = 1e-3;
        point.iter().all(|side| (-SLACK..=1.0 + SLACK).contains(side))
    }

    /// Whether all of a frame on the whole canvas as it is turned on screen shows the photo (REQ-029-10).
    pub fn frame_shows_photo(&self, [x, y, w, h]: [f32; 4]) -> bool {
        [[x, y], [x + w, y], [x, y + h], [x + w, y + h]]
            .into_iter()
            .all(|corner| self.shows_photo(unturn(corner, self.quarter_turns)))
    }

    /// The largest crop of `ratio` (width per height in pixels, on the unturned canvas), centered, that shows only the
    /// photo (REQ-029-10).
    pub fn largest_crop(&self, ratio: f32) -> [f32; 4] {
        let ratio = if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else {
            self.aspect
        };
        let (c, s) = self.turn();
        let (c, s) = (c.abs(), s.abs());
        let [sx, sy] = self.canvas_scale();
        // Half sizes in units of the original's height: corners turned back must stay within its half sizes.
        let (half_w, half_h) = (self.aspect / 2.0, 0.5);
        let a = (half_w / (c + s / ratio)).min(half_h / (s + c / ratio));
        let b = a / ratio;
        // Within rounding of the whole side, the whole side.
        let side = |n: f32| if n > 1.0 - 1e-4 { 1.0 } else { n };
        let (w, h) = (side(2.0 * a / (self.aspect * sx)), side(2.0 * b / sy));
        [(1.0 - w) / 2.0, (1.0 - h) / 2.0, w, h]
    }

    /// The crop's width per height in pixels, on the unturned canvas.
    pub fn crop_ratio(&self) -> f32 {
        let [sx, sy] = self.canvas_scale();
        (self.crop[2] * self.aspect * sx) / (self.crop[3] * sy)
    }

    /// The crop in pixels of the canvas of an upright original of this size: `x, y, w, h`, at least one pixel and
    /// inside it.
    pub fn crop_pixels(&self, width: u32, height: u32) -> [u32; 4] {
        let (width, height) = self.canvas_size(width, height);
        let side = |start: f32, size: f32, whole: u32| {
            let whole_f = whole as f32;
            let first = ((start * whole_f).round() as u32).min(whole.saturating_sub(1));
            let end = (((start + size) * whole_f).round() as u32).clamp(first + 1, whole.max(first + 1));
            (first, end - first)
        };
        let (x, w) = side(self.crop[0], self.crop[2], width);
        let (y, h) = side(self.crop[1], self.crop[3], height);
        [x, y, w, h]
    }

    /// The size of the output for an upright original of this size.
    pub fn output_size(&self, width: u32, height: u32) -> (u32, u32) {
        let [_, _, w, h] = self.crop_pixels(width, height);
        if self.quarter_turns % 2 == 1 { (h, w) } else { (w, h) }
    }

    /// The crop as a frame on the whole canvas as it is turned on screen (`domain::crop`).
    pub fn frame(&self) -> [f32; 4] {
        let [x, y, w, h] = self.crop;
        let a = turn(self.quarter_turns, [x, y]);
        let b = turn(self.quarter_turns, [x + w, y + h]);
        [a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()]
    }

    /// The crop for a frame on the whole canvas turned by `quarter_turns`; `None` for the whole canvas.
    pub fn crop_of_frame([x, y, w, h]: [f32; 4], quarter_turns: u8) -> Option<[f32; 4]> {
        let a = unturn([x, y], quarter_turns);
        let b = unturn([x + w, y + h], quarter_turns);
        sanitized_crop([a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()])
    }

    /// The upright original's size, about, from its output's: a saved edit's item knows only that one.
    pub fn original_size(&self, width: u32, height: u32) -> (u32, u32) {
        let (width, height) = if self.quarter_turns % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        };
        let [sx, sy] = self.canvas_scale();
        (
            (width as f32 / self.crop[2] / sx).round() as u32,
            (height as f32 / self.crop[3] / sy).round() as u32,
        )
    }

    /// Where a point of the original (fractions of its size) lies on the output; outside 0–1 if cropped away.
    pub fn to_output(&self, point: [f32; 2]) -> [f32; 2] {
        let [u, v] = self.to_canvas(point);
        let [x, y, w, h] = self.crop;
        turn(self.quarter_turns, [(u - x) / w, (v - y) / h])
    }

    /// Where a point of the output lies on the original.
    pub fn to_original(&self, point: [f32; 2]) -> [f32; 2] {
        let [u, v] = unturn(point, self.quarter_turns);
        let [x, y, w, h] = self.crop;
        self.from_canvas([x + u * w, y + v * h])
    }

    /// A rectangle of the original (`x, y, w, h`) on the output, cut at the crop's edge; `None` if entirely outside.
    /// Straightened, it is the box around the turned rectangle.
    pub fn place(&self, [x, y, w, h]: [f32; 4]) -> Option<[f32; 4]> {
        let corners = [[x, y], [x + w, y], [x, y + h], [x + w, y + h]].map(|corner| self.to_output(corner));
        let fold = |pick: fn(&[f32; 2]) -> f32, min: bool| {
            corners
                .iter()
                .map(pick)
                .fold(if min { f32::INFINITY } else { f32::NEG_INFINITY }, |a, b| {
                    if min { a.min(b) } else { a.max(b) }
                })
        };
        let (left, right) = (fold(|p| p[0], true).max(0.0), fold(|p| p[0], false).min(1.0));
        let (top, bottom) = (fold(|p| p[1], true).max(0.0), fold(|p| p[1], false).min(1.0));
        (right > left && bottom > top).then_some([left, top, right - left, bottom - top])
    }
}

/// A point of the unturned canvas on the canvas turned by `quarter_turns` clockwise.
fn turn(quarter_turns: u8, [u, v]: [f32; 2]) -> [f32; 2] {
    match quarter_turns % 4 {
        1 => [1.0 - v, u],
        2 => [1.0 - u, 1.0 - v],
        3 => [v, 1.0 - u],
        _ => [u, v],
    }
}

/// A point of the canvas turned by `quarter_turns` clockwise on the unturned one.
fn unturn([u, v]: [f32; 2], quarter_turns: u8) -> [f32; 2] {
    match quarter_turns % 4 {
        1 => [v, 1.0 - u],
        2 => [1.0 - u, 1.0 - v],
        3 => [1.0 - v, u],
        _ => [u, v],
    }
}

/// A straightening within ±[`MAX_STRAIGHTEN`], in steps of a tenth of a degree; 0 for an invalid one.
fn sanitized_straighten(degrees: f32) -> f32 {
    if degrees.is_finite() {
        ((degrees.clamp(-MAX_STRAIGHTEN, MAX_STRAIGHTEN) * 10.0).round() / 10.0) + 0.0
    } else {
        0.0
    }
}

/// A photo's width per height, from a hundredth to a hundred; square for an invalid one.
fn sanitized_aspect(aspect: f32) -> f32 {
    if aspect.is_finite() && aspect > 0.0 {
        aspect.clamp(0.01, 100.0)
    } else {
        1.0
    }
}

/// What the adjustments of the panel change: the whole photo, or only its subject or its background (REQ-022-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Part {
    #[default]
    Photo,
    Subject,
    Background,
    /// A mask of the recipe, by its place in the list (REQ-030-01).
    Mask(usize),
}

impl Part {
    pub const ALL: [Part; 3] = [Self::Photo, Self::Subject, Self::Background];

    pub fn label(self) -> &'static str {
        match self {
            Self::Photo => "Photo",
            Self::Subject => "Subject",
            Self::Background => "Background",
            Self::Mask(_) => "Mask",
        }
    }
}

/// Masks a recipe can have (REQ-030-01).
pub const MAX_MASKS: usize = 8;

/// What a mask selects (REQ-030-03 … REQ-030-05). Positions are fractions of the upright, unturned original, as the
/// crop's are; brightness and softness in percent of the encoded scale, hue, range and color softness in degrees of
/// Oklab's color wheel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MaskShape {
    /// Full at `from`, fading to nothing at `to`.
    Linear {
        from: [f32; 2],
        to: [f32; 2],
    },
    Luminance {
        low: f32,
        high: f32,
        feather: f32,
    },
    Color {
        hue: f32,
        range: f32,
        feather: f32,
    },
    /// A painting of the recipe, by its place: brushed strokes, or a filled outline if `freeform`.
    Painted {
        layer: u8,
        freeform: bool,
    },
}

impl MaskShape {
    /// From the top to the middle of the photo: a sky, usually.
    pub const LINEAR: Self = Self::Linear {
        from: [0.5, 0.1],
        to: [0.5, 0.5],
    };
    /// The bright half.
    pub const LUMINANCE: Self = Self::Luminance {
        low: 60.0,
        high: 100.0,
        feather: 15.0,
    };
    /// Greens.
    pub const COLOR: Self = Self::Color {
        hue: 140.0,
        range: 30.0,
        feather: 20.0,
    };

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear { .. } => "Gradient",
            Self::Luminance { .. } => "Luminance",
            Self::Color { .. } => "Color",
            Self::Painted { freeform: false, .. } => "Brush",
            Self::Painted { freeform: true, .. } => "Freeform",
        }
    }

    /// The panel's sliders of this shape: name, value, range, unit (REQ-030-04, REQ-030-05). A gradient has handles.
    pub fn sliders(self) -> Vec<(&'static str, f32, (f32, f32), &'static str)> {
        match self {
            Self::Linear { .. } | Self::Painted { .. } => Vec::new(),
            Self::Luminance { low, high, feather } => vec![
                ("From", low, (0.0, 100.0), " %"),
                ("To", high, (0.0, 100.0), " %"),
                ("Softness", feather, (0.0, 50.0), " %"),
            ],
            Self::Color { hue, range, feather } => vec![
                ("Mask hue", hue, (0.0, 360.0), "°"),
                ("Range", range, (0.0, 180.0), "°"),
                ("Softness", feather, (0.0, 90.0), "°"),
            ],
        }
    }

    /// Sets slider `i` of [`Self::sliders`].
    pub fn set_slider(&mut self, i: usize, value: f32) {
        match self {
            Self::Linear { .. } | Self::Painted { .. } => {}
            Self::Luminance { low, high, feather } => *[low, high, feather][i] = value,
            Self::Color { hue, range, feather } => *[hue, range, feather][i] = value,
        }
    }

    /// Values from a file brought into their ranges (REQ-011-10).
    fn sanitize(&mut self) {
        let fit = |value: &mut f32, low: f32, high: f32, or: f32| {
            *value = if value.is_finite() { value.clamp(low, high) } else { or };
        };
        match self {
            Self::Linear { from, to } => {
                for c in from.iter_mut().chain(to.iter_mut()) {
                    fit(c, -1.0, 2.0, 0.5);
                }
            }
            Self::Luminance { low, high, feather } => {
                fit(low, 0.0, 100.0, 0.0);
                fit(high, 0.0, 100.0, 100.0);
                fit(feather, 0.0, 100.0, 0.0);
            }
            Self::Color { hue, range, feather } => {
                fit(hue, 0.0, 360.0, 0.0);
                fit(range, 0.0, 180.0, 30.0);
                fit(feather, 0.0, 90.0, 0.0);
            }
            Self::Painted { .. } => {}
        }
    }

    /// How much of the pixel `rgb` as stored, at `uv` of a photo `size` pixels large, this selects: 0 to 1.
    pub fn weight(self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> f32 {
        match self {
            Self::Linear { from, to } => {
                let along = [(to[0] - from[0]) * size[0], (to[1] - from[1]) * size[1]];
                let length = along[0] * along[0] + along[1] * along[1];
                if length <= 0.0 {
                    return 0.0;
                }
                let at = [(uv[0] - from[0]) * size[0], (uv[1] - from[1]) * size[1]];
                1.0 - smoothstep(0.0, 1.0, (at[0] * along[0] + at[1] * along[1]) / length)
            }
            Self::Luminance { low, high, feather } => {
                let value = to_encoded(luminance(rgb.map(to_linear)).max(0.0)) * 100.0;
                let feather = feather.max(0.5);
                smoothstep(low - feather, low, value) * (1.0 - smoothstep(high, high + feather, value))
            }
            Self::Color { hue, range, feather } => {
                let [_, a, b] = to_oklab(rgb.map(to_linear));
                let chroma = (a * a + b * b).sqrt();
                // The short way around the wheel.
                let away = ((b.atan2(a).to_degrees() - hue + 180.0).rem_euclid(360.0) - 180.0).abs();
                (1.0 - smoothstep(range, range + feather.max(0.5), away)) * smoothstep(0.02, 0.06, chroma)
            }
            // `MaskParams::weight` reads the painting.
            Self::Painted { .. } => 0.0,
        }
    }

    /// The numbers the shader's `LocalMask` holds: its kind, then the shape's.
    pub fn numbers(self) -> [f32; 5] {
        match self {
            Self::Linear { from, to } => [0.0, from[0], from[1], to[0], to[1]],
            Self::Luminance { low, high, feather } => [1.0, low / 100.0, high / 100.0, feather / 100.0, 0.0],
            Self::Color { hue, range, feather } => [2.0, hue, range, feather, 0.0],
            Self::Painted { layer, .. } => [3.0, f32::from(layer), 0.0, 0.0, 0.0],
        }
    }
}

/// A mask of a recipe: what it selects, and the light and color it gives that (REQ-030-02).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalMask {
    pub shape: MaskShape,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub recipe: Box<Recipe>,
}

impl LocalMask {
    pub fn new(shape: MaskShape) -> Self {
        Self {
            shape,
            invert: false,
            recipe: Box::default(),
        }
    }
}

/// The masks of a recipe from a file; a kind only a newer version knows is left out (REQ-030-08).
fn masks_known<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<LocalMask>, D::Error> {
    let masks = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(masks
        .into_iter()
        .filter_map(|mask| serde_json::from_value(mask).ok())
        .collect())
}

/// A mask's numbers in [`Params`].
#[derive(Debug, Clone, PartialEq)]
pub struct MaskParams {
    pub shape: MaskShape,
    pub invert: bool,
    pub params: Box<Params>,
    /// What the recipe's paintings select, once [`Params::with_paintings`] drew them.
    pub paintings: Vec<Arc<Mask>>,
}

impl MaskParams {
    /// How much of the pixel this mask selects (REQ-030-02, REQ-030-06).
    pub fn weight(&self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> f32 {
        let weight = match self.shape {
            MaskShape::Painted { layer, .. } => self
                .paintings
                .get(usize::from(layer))
                .map_or(0.0, |painting| painting.sample(uv)),
            shape => shape.weight(rgb, uv, size),
        };
        if self.invert { 1.0 - weight } else { weight }
    }
}

/// How much each spot of a photo belongs to its subject, 0 to 1, on a grid laid over the whole photo (REQ-022-03).
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    width: u32,
    height: u32,
    values: Vec<f32>,
}

impl Mask {
    /// `None` unless there are `width × height` values.
    pub fn new(width: u32, height: u32, values: Vec<f32>) -> Option<Self> {
        (width > 0 && height > 0 && values.len() == width as usize * height as usize).then_some(Self {
            width,
            height,
            values,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row by row from the top left.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// The value at `uv` (0–1 across the photo), bilinear between the grid's cell centers, as a GPU samples it.
    pub fn sample(&self, uv: [f32; 2]) -> f32 {
        let at = |c: f32, side: u32| (c * side as f32 - 0.5).clamp(0.0, (side - 1) as f32);
        let (x, y) = (at(uv[0], self.width), at(uv[1], self.height));
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (tx, ty) = (x - x0 as f32, y - y0 as f32);
        let value = |x: u32, y: u32| self.values[(y * self.width + x) as usize];
        let top = value(x0, y0) * (1.0 - tx) + value(x1, y0) * tx;
        let bottom = value(x0, y1) * (1.0 - tx) + value(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// How a photo is blurred (REQ-023-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlurKind {
    #[default]
    None,
    /// Everything but the subject (spec 022).
    Background,
    /// All but a horizontal band through the focus.
    TiltShift,
    /// All but a round spot around the focus.
    Radial,
}

impl BlurKind {
    pub const ALL: [BlurKind; 4] = [Self::None, Self::Background, Self::TiltShift, Self::Radial];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Background => "Behind subject",
            Self::TiltShift => "Tilt-shift",
            Self::Radial => "Radial",
        }
    }

    /// Whether it has a focus to place, a size and a transition.
    pub fn has_focus(self) -> bool {
        matches!(self, Self::TiltShift | Self::Radial)
    }
}

fn look_or_none<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<crate::look::LookChoice>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

fn blur_kind_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BlurKind, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// The strongest blur, as σ in parts of the photo's longer side.
const MAX_BLUR: f32 = 0.015;
/// The σ, in pixels of a pyramid's base, of one level of [`Pyramid`] (a 2 × 2 box per level, then a B-spline).
pub const BLUR_SIGMA_PER_LEVEL: f32 = 0.65;

/// The level of detail of a [`Pyramid`] with `levels` levels for a blur of `sigma` pixels of its base (REQ-023-04).
pub fn blur_lod(sigma: f32, levels: usize) -> f32 {
    if sigma <= 0.0 {
        return 0.0;
    }
    (sigma / BLUR_SIGMA_PER_LEVEL)
        .log2()
        .clamp(0.0, levels.saturating_sub(1) as f32)
}

/// A photo halved again and again by 2 × 2 boxes while its shorter side is at least 32 px, as the GPU's mip chain
/// (`app::textures::mip_chain`); blurs sample it with a cubic B-spline (REQ-023-04).
#[derive(Debug, Clone)]
pub struct Pyramid {
    /// From the second level on: the first is the photo itself.
    levels: Vec<(usize, usize, Vec<[f32; 4]>)>,
}

impl Pyramid {
    /// From the photo's RGBA values (as stored, 0–1), row by row.
    pub fn new(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> Self {
        let mut levels = Vec::new();
        let (mut w, mut h) = (width as usize, height as usize);
        let mut source = pixels;
        while w.min(h) >= 32 {
            let (nw, nh) = (w / 2, h / 2);
            let half: Vec<[f32; 4]> = (0..nw * nh)
                .map(|i| {
                    let (x, y) = (2 * (i % nw), 2 * (i / nw));
                    let at = |x: usize, y: usize| source[y * w + x];
                    let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1));
                    std::array::from_fn(|k| (a[k] + b[k] + c[k] + d[k]) / 4.0)
                })
                .collect();
            levels.push((nw, nh, half.clone()));
            source = half;
            (w, h) = (nw, nh);
        }
        Self { levels }
    }

    /// Levels, the photo itself included.
    pub fn levels(&self) -> usize {
        self.levels.len() + 1
    }

    /// The photo at `uv` blurred to level of detail `lod`: `sharp` (the pixel itself) at 0, between levels a mix.
    pub fn blurred(&self, sharp: [f32; 4], uv: [f32; 2], lod: f32) -> [f32; 4] {
        let lod = lod.clamp(0.0, self.levels.len() as f32);
        if lod <= 0.0 {
            return sharp;
        }
        let low = lod.floor() as usize;
        let t = lod - low as f32;
        let below = if low == 0 { sharp } else { self.bspline(low, uv) };
        if t <= 0.0 {
            return below;
        }
        let above = self.bspline(low + 1, uv);
        std::array::from_fn(|k| below[k] + (above[k] - below[k]) * t)
    }

    /// Between the levels around `lod` (1 or more) at `uv`, each read bilinearly, clamped at the edges: what a GPU's
    /// trilinear sampler reads (REQ-023-05).
    pub fn trilinear(&self, uv: [f32; 2], lod: f32) -> [f32; 4] {
        let lod = lod.clamp(1.0, self.levels.len() as f32);
        let low = lod.floor() as usize;
        let t = lod - low as f32;
        let below = self.bilinear(low, uv);
        if t <= 0.0 {
            return below;
        }
        let above = self.bilinear(low + 1, uv);
        std::array::from_fn(|k| below[k] + (above[k] - below[k]) * t)
    }

    /// Level `level` (1 or more) at `uv`, between its four nearest pixels, clamped at the edges.
    fn bilinear(&self, level: usize, uv: [f32; 2]) -> [f32; 4] {
        let (w, h, pixels) = &self.levels[level - 1];
        let (px, py) = (uv[0] * *w as f32 - 0.5, uv[1] * *h as f32 - 0.5);
        let (ix, iy) = (px.floor(), py.floor());
        let (fx, fy) = (px - ix, py - iy);
        let at = |x: f32, y: f32| {
            let x = (x as i64).clamp(0, *w as i64 - 1) as usize;
            let y = (y as i64).clamp(0, *h as i64 - 1) as usize;
            pixels[y * w + x]
        };
        let (a, b, c, d) = (at(ix, iy), at(ix + 1.0, iy), at(ix, iy + 1.0), at(ix + 1.0, iy + 1.0));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }

    /// Level `level` (1 or more) at `uv`, a cubic B-spline over its pixels, clamped at the edges.
    fn bspline(&self, level: usize, uv: [f32; 2]) -> [f32; 4] {
        let (w, h, pixels) = &self.levels[level - 1];
        let (px, py) = (uv[0] * *w as f32 - 0.5, uv[1] * *h as f32 - 0.5);
        let (ix, iy) = (px.floor(), py.floor());
        let (wx, wy) = (bspline_weights(px - ix), bspline_weights(py - iy));
        let mut out = [0.0; 4];
        for (j, wy) in wy.into_iter().enumerate() {
            let y = (iy as i64 - 1 + j as i64).clamp(0, *h as i64 - 1) as usize;
            for (i, wx) in wx.into_iter().enumerate() {
                let x = (ix as i64 - 1 + i as i64).clamp(0, *w as i64 - 1) as usize;
                let pixel = pixels[y * w + x];
                for k in 0..4 {
                    out[k] += pixel[k] * wx * wy;
                }
            }
        }
        out
    }
}

/// Taps of the lens blur's disc (REQ-023-05), on a golden-angle spiral that covers it evenly.
pub const LENS_TAPS: u32 = 48;
/// A lens disc's radius per σ of the soft blur, so the rest of the photo blurs about as much.
pub const LENS_RADIUS_PER_SIGMA: f32 = 2.0;
/// How much more than a midtone a full highlight weighs, so it spreads into a bright disc.
pub const LENS_BOOST: f32 = 40.0;
/// The angle between a spiral's taps.
const GOLDEN_ANGLE: f32 = 2.399_963;

/// What a pixel (as stored, 0–1) weighs in the lens blur: highlights far more than the rest (REQ-023-05).
pub fn lens_weight([r, g, b]: [f32; 3]) -> f32 {
    let t = ((r.max(g).max(b) - 0.7) / 0.3).clamp(0.0, 1.0);
    1.0 + LENS_BOOST * t * t
}

/// A pixel times its lens weight, and the weight: what a lens pyramid holds, so halving keeps a light's share.
pub fn lens_weighted([r, g, b, _]: [f32; 4]) -> [f32; 4] {
    let w = lens_weight([r, g, b]);
    [r * w, g * w, b * w, w]
}

/// The level of detail a lens tap samples for a disc of `radius` pixels: wide enough that the taps cover the disc
/// without gaps, and never the base itself, which a lens pyramid needn't hold (REQ-023-05).
pub fn lens_lod(radius: f32, levels: usize) -> f32 {
    let spacing = radius * (std::f32::consts::PI / LENS_TAPS as f32).sqrt();
    blur_lod(spacing, levels).max(1.0)
}

/// Where tap `i` of a lens disc lies, in parts of its radius.
pub fn lens_tap(i: u32) -> [f32; 2] {
    let r = ((i as f32 + 0.5) / LENS_TAPS as f32).sqrt();
    let angle = i as f32 * GOLDEN_ANGLE;
    [r * angle.cos(), r * angle.sin()]
}

/// The lens blur at `uv`: the mean of the disc of `radius` pixels of the base (of `size` pixels), weighted so lights
/// spread into evenly lit discs. `weighted` holds [`lens_weighted`] pixels. `None` for a disc too small to sample, or
/// a photo too small for a pyramid (REQ-023-05).
pub fn lens_blurred(weighted: &Pyramid, uv: [f32; 2], radius: f32, size: [f32; 2]) -> Option<[f32; 3]> {
    if radius < 1.0 || weighted.levels() < 2 {
        return None;
    }
    let lod = lens_lod(radius, weighted.levels());
    let mut sum = [0.0; 4];
    for i in 0..LENS_TAPS {
        let [x, y] = lens_tap(i);
        let tap = [uv[0] + x * radius / size[0], uv[1] + y * radius / size[1]];
        let value = weighted.trilinear(tap, lod);
        for k in 0..4 {
            sum[k] += value[k];
        }
    }
    (sum[3] > 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
}

/// A pixel `sharp` at `uv` blurred by σ `sigma` pixels of the pyramids' base (of `size` pixels): in discs from `lens`
/// (of [`lens_weighted`] pixels) where given and the disc is large enough, else softly from `pyramid` (REQ-023-04,
/// REQ-023-05). Transparency stays sharp.
pub fn blur_pixel(
    pyramid: &Pyramid,
    lens: Option<&Pyramid>,
    sharp: [f32; 4],
    uv: [f32; 2],
    sigma: f32,
    size: [f32; 2],
) -> [f32; 4] {
    if let Some(lens) = lens
        && let Some([r, g, b]) = lens_blurred(lens, uv, sigma * LENS_RADIUS_PER_SIGMA, size)
    {
        return [r, g, b, sharp[3]];
    }
    pyramid.blurred(sharp, uv, blur_lod(sigma, pyramid.levels()))
}

/// The cubic B-spline's weights of the four pixels around a position `f` (0–1) past the second.
fn bspline_weights(f: f32) -> [f32; 4] {
    let (f2, f3) = (f * f, f * f * f);
    [
        (1.0 - f).powi(3) / 6.0,
        (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0,
        (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0,
        f3 / 6.0,
    ]
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
    Hue,
    Vibrance,
    Saturation,
    Grain,
    GrainSize,
    Vignette,
    VignetteMidpoint,
    Blur,
    BlurSize,
    BlurFeather,
}

impl Adjustment {
    pub const ALL: [Adjustment; 19] = [
        Self::Exposure,
        Self::Brightness,
        Self::Contrast,
        Self::Highlights,
        Self::Shadows,
        Self::Whites,
        Self::Blacks,
        Self::Temperature,
        Self::Tint,
        Self::Hue,
        Self::Vibrance,
        Self::Saturation,
        Self::Blur,
        Self::BlurSize,
        Self::BlurFeather,
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
            Self::Temperature | Self::Tint | Self::Hue | Self::Vibrance | Self::Saturation => Section::Color,
            Self::Grain
            | Self::GrainSize
            | Self::Vignette
            | Self::VignetteMidpoint
            | Self::Blur
            | Self::BlurSize
            | Self::BlurFeather => Section::Effects,
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
            Self::Hue => "Hue",
            Self::Vibrance => "Vibrance",
            Self::Saturation => "Saturation",
            Self::Grain => "Grain",
            Self::GrainSize => "Grain size",
            Self::Vignette => "Vignette",
            Self::VignetteMidpoint => "Midpoint",
            Self::Blur => "Strength",
            Self::BlurSize => "Focus size",
            Self::BlurFeather => "Transition",
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Exposure => (-5.0, 5.0),
            Self::Hue => (-180.0, 180.0),
            Self::Grain
            | Self::GrainSize
            | Self::VignetteMidpoint
            | Self::Blur
            | Self::BlurSize
            | Self::BlurFeather => (0.0, 100.0),
            _ => (-100.0, 100.0),
        }
    }

    pub fn neutral(self) -> f32 {
        match self {
            Self::GrainSize => 25.0,
            Self::VignetteMidpoint | Self::BlurFeather => 50.0,
            Self::BlurSize => 30.0,
            _ => 0.0,
        }
    }

    /// Whether the subject and the background have it of their own: light and color (REQ-022-01).
    pub fn in_parts(self) -> bool {
        matches!(self.section(), Section::Light | Section::Color)
    }

    /// False for the shape of an effect (grain size, vignette midpoint), which alone changes nothing.
    pub fn changes_pixels(self) -> bool {
        !matches!(
            self,
            Self::GrainSize | Self::VignetteMidpoint | Self::BlurSize | Self::BlurFeather
        )
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
            Self::Hue => recipe.hue,
            Self::Vibrance => recipe.vibrance,
            Self::Saturation => recipe.saturation,
            Self::Grain => recipe.grain,
            Self::GrainSize => recipe.grain_size,
            Self::Vignette => recipe.vignette,
            Self::VignetteMidpoint => recipe.vignette_midpoint,
            Self::Blur => recipe.blur,
            Self::BlurSize => recipe.blur_size,
            Self::BlurFeather => recipe.blur_feather,
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
            Self::Hue => &mut recipe.hue,
            Self::Vibrance => &mut recipe.vibrance,
            Self::Saturation => &mut recipe.saturation,
            Self::Grain => &mut recipe.grain,
            Self::GrainSize => &mut recipe.grain_size,
            Self::Vignette => &mut recipe.vignette,
            Self::VignetteMidpoint => &mut recipe.vignette_midpoint,
            Self::Blur => &mut recipe.blur,
            Self::BlurSize => &mut recipe.blur_size,
            Self::BlurFeather => &mut recipe.blur_feather,
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
    /// The turn around the color wheel, in radians.
    pub hue: f32,
    pub vignette: f32,
    pub vignette_start: f32,
    /// The geometry, whose output's center and edges the vignette takes (REQ-029-04).
    pub vignette_geometry: Geometry,
    pub grain: f32,
    /// Grain cell size in pixels of the full photo.
    pub grain_cell: f32,
    /// A film look's grain: strongest in the midtones (REQ-028-06).
    pub grain_midtones: bool,
    /// How much the grain differs between the color channels, 0–1 (REQ-028-06).
    pub grain_color: f32,
    /// The masks' numbers, in order (REQ-030-02).
    pub masks: Vec<MaskParams>,
    /// How strongly bright lights glow red around them, for a film look (REQ-028-08).
    pub halation: f32,
    /// What a film look does to each hue, at its strength: turn in radians, chroma's factor, lightness added
    /// (REQ-028-07). `None` where it changes nothing.
    pub hue_nodes: Option<[[f32; 3]; crate::look::HUE_NODES]>,
    /// The curves, all channels composed into one table per channel; `None` for straight curves.
    pub curves: Option<Vec<[f32; 3]>>,
    pub blur_kind: BlurKind,
    /// The strongest blur, as σ in parts of the longer side; 0 for none.
    pub blur: f32,
    /// Half the sharp area's size and the transition's width, in parts of the longer side.
    pub blur_size: f32,
    pub blur_feather: f32,
    pub blur_focus: [f32; 2],
    /// Discs instead of a soft blur (REQ-023-05).
    pub blur_lens: bool,
    /// The subject's and the background's own numbers (REQ-022-03).
    pub subject: Option<Box<Params>>,
    pub background: Option<Box<Params>>,
}

impl Params {
    pub fn new(recipe: &Recipe) -> Self {
        // A film look adds to the adjustments, and its curves go before the recipe's own (REQ-028-04).
        let look = recipe.look.map(|choice| choice.look);
        let recipe_look = recipe.look;
        let hue_nodes = recipe.look.and_then(|choice| {
            let strength = choice.strength / 100.0;
            let nodes = choice.look.hue_nodes();
            (strength > 0.0 && nodes != [[0.0, 1.0, 0.0]; crate::look::HUE_NODES]).then(|| {
                nodes.map(|[turn, chroma, light]| {
                    [
                        turn.to_radians() * strength,
                        1.0 + (chroma - 1.0) * strength,
                        light * strength,
                    ]
                })
            })
        });
        let (folded, look_curves) = crate::look::fold(recipe);
        let recipe = &folded;
        let curves = (!recipe.curves.is_identity() || look_curves.is_some()).then(|| {
            let c = &recipe.curves;
            let channel = |curves: &Curves, x: f32| {
                let all = curves.all.eval(x);
                [curves.red.eval(all), curves.green.eval(all), curves.blue.eval(all)]
            };
            (0..CURVE_SAMPLES)
                .map(|i| {
                    let x = i as f32 / (CURVE_SAMPLES - 1) as f32;
                    let looked = look_curves.as_ref().map_or([x; 3], |look| channel(look, x));
                    std::array::from_fn(|k| channel(c, looked[k])[k])
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
            hue: recipe.hue.to_radians(),
            vignette: recipe.vignette / 100.0,
            vignette_start: recipe.vignette_midpoint / 100.0 * 0.8,
            vignette_geometry: recipe.geometry(),
            grain: recipe.grain / 100.0,
            grain_cell: 1.0 + recipe.grain_size / 100.0 * 3.0,
            grain_midtones: look.is_some(),
            grain_color: look.map_or(0.0, crate::look::Look::grain_color),
            hue_nodes,
            masks: recipe
                .masks
                .iter()
                .map(|mask| MaskParams {
                    shape: mask.shape,
                    invert: mask.invert,
                    params: Box::new(Params::new(&mask.recipe)),
                    paintings: Vec::new(),
                })
                .collect(),
            halation: recipe_look.map_or(0.0, |choice| choice.look.halation() * choice.strength / 100.0),
            curves,
            blur_kind: recipe.blur_kind,
            blur: if recipe.blur_kind == BlurKind::None {
                0.0
            } else {
                recipe.blur / 100.0 * MAX_BLUR
            },
            blur_size: recipe.blur_size / 100.0 * 0.5,
            blur_feather: 0.02 + recipe.blur_feather / 100.0 * 0.5,
            blur_focus: recipe.blur_focus,
            blur_lens: recipe.blur_lens,
            subject: recipe.subject.as_deref().map(|part| Box::new(Self::new(part))),
            background: recipe.background.as_deref().map(|part| Box::new(Self::new(part))),
        }
    }

    /// These numbers with `paintings`, what the recipe's paintings select (REQ-030-12). [`Self::new`] leaves them out, as
    /// it runs every frame: without them, painted shapes select nothing.
    pub fn with_paintings(mut self, paintings: &[Arc<Mask>]) -> Self {
        for mask in &mut self.masks {
            mask.paintings = paintings.to_vec();
        }
        self
    }

    /// One pixel: `rgb` as stored (sRGB-encoded, 0–1), at `uv` (0–1 across the photo) of a photo `size` pixels large
    /// at full resolution. Returns encoded values in 0–1.
    pub fn pixel(&self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> [f32; 3] {
        self.pixel_glowing(rgb, [0.0; 3], uv, size, None)
    }

    /// [`Self::pixel`] with the subject's and the background's own adjustments, `subject` being how much the pixel
    /// belongs to the subject (REQ-022-03).
    pub fn pixel_masked(&self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2], subject: f32) -> [f32; 3] {
        self.pixel_glowing(rgb, [0.0; 3], uv, size, Some(subject))
    }

    /// One pixel with the [`Self::halation_glow`] it gets, and with its parts if `subject` is given.
    pub fn pixel_glowing(
        &self,
        rgb: [f32; 3],
        glow: [f32; 3],
        uv: [f32; 2],
        size: [f32; 2],
        subject: Option<f32>,
    ) -> [f32; 3] {
        let mut lin = self.develop(rgb.map(to_linear));
        if let Some(subject) = subject {
            let part = |params: &Option<Box<Params>>| params.as_ref().map_or(lin, |params| params.develop(lin));
            let (inside, outside) = (part(&self.subject), part(&self.background));
            lin = std::array::from_fn(|c| outside[c] + (inside[c] - outside[c]) * subject);
        }
        for mask in &self.masks {
            let weight = mask.weight(rgb, uv, size);
            let developed = mask.params.develop(lin);
            lin = std::array::from_fn(|c| lin[c] + (developed[c] - lin[c]) * weight);
        }
        self.finish(std::array::from_fn(|c| lin[c] + glow[c]), uv, size)
    }

    /// The blur that halation gathers the light with, as σ in pixels of a photo `size` large (REQ-028-08).
    pub fn halation_sigma(size: [f32; 2]) -> f32 {
        HALATION_SIZE * size[0].max(size[1])
    }

    /// The red glow on linear light of a pixel, `sharp` and `blurred` being it and its surroundings as stored
    /// (REQ-028-08): only where the surroundings are brighter highlights than it, so that an evenly bright sky keeps
    /// its color.
    pub fn halation_glow(&self, sharp: [f32; 3], blurred: [f32; 3]) -> [f32; 3] {
        let light = |rgb: [f32; 3]| smoothstep(0.3, 0.9, rgb[0].max(rgb[1]).max(rgb[2]));
        let glow = (light(blurred) - light(sharp)).max(0.0) * self.halation;
        HALATION_COLOR.map(|c| c * glow)
    }

    /// The blur at `uv` of a photo `size` pixels large, as σ in its pixels, `subject` being how much the spot belongs to
    /// the subject (REQ-023-02, REQ-023-03). The photo is first blurred so, then developed.
    pub fn blur_sigma(&self, uv: [f32; 2], size: [f32; 2], subject: f32) -> f32 {
        if self.blur <= 0.0 {
            return 0.0;
        }
        let longer = size[0].max(size[1]);
        let offset = [
            (uv[0] - self.blur_focus[0]) * size[0],
            (uv[1] - self.blur_focus[1]) * size[1],
        ];
        let weight = match self.blur_kind {
            BlurKind::None => 0.0,
            BlurKind::Background => 1.0 - subject.clamp(0.0, 1.0),
            BlurKind::TiltShift | BlurKind::Radial => {
                let distance = if self.blur_kind == BlurKind::TiltShift {
                    offset[1].abs()
                } else {
                    (offset[0] * offset[0] + offset[1] * offset[1]).sqrt()
                } / longer;
                smoothstep(self.blur_size, self.blur_size + self.blur_feather, distance)
            }
        };
        weight * self.blur * longer
    }

    /// White balance, exposure, tone, curves and chroma, on linear light.
    fn develop(&self, lin: [f32; 3]) -> [f32; 3] {
        let mut lin = self.light(lin);
        if let Some(table) = &self.curves {
            for (i, c) in lin.iter_mut().enumerate() {
                *c = to_linear(lookup(table, i, to_encoded(c.max(0.0)).min(1.0)));
            }
        }
        if self.saturation != 0.0 || self.vibrance != 0.0 || self.hue != 0.0 {
            lin = self.chroma(lin);
        }
        if let Some(nodes) = &self.hue_nodes {
            lin = turn_hues(nodes, lin);
        }
        lin
    }

    /// Vignette and grain, over the whole photo; returns encoded values in 0–1.
    fn finish(&self, mut lin: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> [f32; 3] {
        if self.vignette != 0.0 {
            let [u, v] = self.vignette_geometry.to_output(uv);
            let (u, v) = ((u - 0.5) * 2.0, (v - 0.5) * 2.0);
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
            let (x, y) = (uv[0] * size[0] / self.grain_cell, uv[1] * size[1] / self.grain_cell);
            let n = value_noise(x, y);
            // Each color channel's own noise, as a color film's three layers have.
            let own = if self.grain_color > 0.0 {
                [value_noise(x + 37.0, y + 17.0), n, value_noise(x + 59.0, y + 91.0)]
            } else {
                [n; 3]
            };
            let amount = if self.grain_midtones {
                let v = (out[0] + out[1] + out[2]) / 3.0;
                self.grain * (0.35 + 0.65 * 4.0 * v.clamp(0.0, 1.0) * (1.0 - v.clamp(0.0, 1.0)))
            } else {
                self.grain
            };
            for (c, own) in out.iter_mut().zip(own) {
                *c += (n + (own - n) * self.grain_color) * amount * 0.2;
            }
        }
        out.map(|c| c.clamp(0.0, 1.0))
    }

    /// White balance, exposure and tone: the steps before the curves, on linear light.
    fn light(&self, mut lin: [f32; 3]) -> [f32; 3] {
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
            for (c, value) in self.light(rgb.map(to_linear)).into_iter().enumerate() {
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

    /// Saturation and vibrance as chroma scaling in OkLab; vibrance favours muted colors and spares skin tones. Then
    /// the hue turns the colors around OkLab's lightness axis, keeping lightness and chroma (REQ-021-20).
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
        let (a, b) = (a * factor, b * factor);
        let (sin, cos) = self.hue.sin_cos();
        from_oklab([l, a * cos - b * sin, a * sin + b * cos])
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

/// The size of halation's blur, in parts of the photo's longer side (REQ-028-08).
const HALATION_SIZE: f32 = 0.006;
/// Halation's color, on linear light: the red layer catches most of what scatters back.
const HALATION_COLOR: [f32; 3] = [1.0, 0.22, 0.06];

/// A film look's change per hue (REQ-028-07), blended between the two nearest nodes. Greys have no hue, so the lightness
/// only applies as a color gets some chroma.
fn turn_hues(nodes: &[[f32; 3]; crate::look::HUE_NODES], lin: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = to_oklab(lin);
    let at = (b.atan2(a) / std::f32::consts::TAU).rem_euclid(1.0) * crate::look::HUE_NODES as f32;
    let (i, f) = (at.floor() as usize % crate::look::HUE_NODES, at.fract());
    let (low, high) = (nodes[i], nodes[(i + 1) % crate::look::HUE_NODES]);
    let [turn, chroma, light] = std::array::from_fn(|k| low[k] + (high[k] - low[k]) * f);
    let colorful = ((a * a + b * b).sqrt() / 0.1).min(1.0);
    let (sin, cos) = turn.sin_cos();
    let (a, b) = (a * chroma, b * chroma);
    from_oklab([l + light * colorful, a * cos - b * sin, a * sin + b * cos])
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
    #[test]
    fn req_029_09_scales_the_long_edge_down_and_never_up() {
        assert_eq!(scaled_size(4000, 3000, Some(1920)), (1920, 1440));
        assert_eq!(scaled_size(3000, 4000, Some(1000)), (750, 1000));
        assert_eq!(scaled_size(4000, 3000, None), (4000, 3000));
        // Not larger than the photo, also after a crop made it smaller.
        assert_eq!(scaled_size(1200, 900, Some(1920)), (1200, 900));
        // A thin panorama keeps at least a pixel.
        assert_eq!(scaled_size(10000, 10, Some(100)), (100, 1));
    }

    #[test]
    fn req_029_09_req_021_21_size_and_quality_are_changes_and_round_trip() {
        let mut recipe = Recipe {
            long_edge: Some(1920),
            ..Recipe::default()
        };
        assert!(!recipe.is_neutral());
        assert_eq!(Recipe::from_json(&recipe.to_json()), Some(recipe.clone()));
        recipe.long_edge = None;
        recipe.quality = Some(60);
        assert!(!recipe.is_neutral());
        assert_eq!(recipe.quality(), 60);
        assert_eq!(Recipe::from_json(&recipe.to_json()), Some(recipe.clone()));
        // The default quality is no change, and nothing goes below the lowest.
        recipe.quality = Some(DEFAULT_QUALITY);
        recipe.tidy();
        assert!(recipe.is_neutral());
        recipe.quality = Some(1);
        recipe.long_edge = Some(2);
        recipe.tidy();
        assert_eq!(
            (recipe.quality, recipe.long_edge),
            (Some(MIN_QUALITY), Some(MIN_LONG_EDGE))
        );
    }

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
        // Blur needs a kind too (REQ-023-01).
        for adjustment in Adjustment::ALL
            .into_iter()
            .filter(|a| a.changes_pixels() && *a != Adjustment::Blur)
        {
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
    fn req_022_03_parts_change_only_their_part_on_top_of_the_photo() {
        let rgb = [0.6, 0.3, 0.2];
        let photo = with(Adjustment::Exposure, 0.5);
        let mut recipe = photo.clone();
        Adjustment::Exposure.set(recipe.part_mut(Part::Subject), 1.0);
        Adjustment::Saturation.set(recipe.part_mut(Part::Background), -100.0);
        assert!(!recipe.is_neutral());
        let params = Params::new(&recipe);
        let whole = render(&photo, rgb);

        // The subject: the photo's exposure, then its own.
        let subject = params.pixel_masked(rgb, MID, SIZE, 1.0);
        assert!(
            (to_linear(subject[1]) - 2.0 * to_linear(whole[1])).abs() < 1e-3,
            "{subject:?}"
        );
        // The background: grey, at the photo's exposure.
        let background = params.pixel_masked(rgb, MID, SIZE, 0.0);
        assert!((background[0] - background[2]).abs() < 1e-3, "{background:?}");
        // Across the edge, in between.
        let edge = params.pixel_masked(rgb, MID, SIZE, 0.5);
        assert!(edge[1] > background[1] && edge[1] < subject[1]);
        // Without a mask, the photo alone.
        assert!(close(params.pixel(rgb, MID, SIZE), whole));

        // Parts at 0 change nothing and aren't kept.
        let mut neutral = Recipe::default();
        Adjustment::Contrast.set(neutral.part_mut(Part::Subject), 0.0);
        neutral.tidy();
        assert_eq!(neutral, Recipe::default());
        assert!(close(
            Params::new(&neutral).pixel_masked(rgb, MID, SIZE, 1.0),
            render(&neutral, rgb)
        ));
        // Only light and color have parts.
        assert!(Adjustment::Exposure.in_parts() && Adjustment::Tint.in_parts());
        assert!(!Adjustment::Grain.in_parts() && !Adjustment::Vignette.in_parts());
    }

    #[test]
    fn req_022_03_the_mask_fades_across_the_subjects_edge() {
        let mask = Mask::new(2, 1, vec![0.0, 1.0]).unwrap();
        assert_eq!(mask.sample([0.25, 0.5]), 0.0);
        assert_eq!(mask.sample([0.75, 0.5]), 1.0);
        assert!((mask.sample([0.5, 0.5]) - 0.5).abs() < 1e-6);
        assert_eq!(mask.sample([-1.0, 2.0]), 0.0);
        assert!(Mask::new(2, 2, vec![0.0]).is_none());
    }

    #[test]
    fn req_022_05_parts_are_saved_with_the_recipe() {
        let mut recipe = Recipe::default();
        Adjustment::Shadows.set(recipe.part_mut(Part::Subject), 40.0);
        Adjustment::Temperature.set(recipe.part_mut(Part::Background), -20.0);
        let json = recipe.to_json();
        assert_eq!(Recipe::from_json(&json), Some(recipe.clone()));
        // Older recipes have none.
        let old = Recipe::from_json(r#"{"version":1,"exposure":0.5}"#).unwrap();
        assert!(old.part(Part::Subject).is_none() && !old.has_parts());
        // A hostile part: only light and color are kept, no nested parts; a neutral one is dropped.
        let hostile = Recipe::from_json(
            r#"{"subject":{"exposure":9,"grain":50,"curves":{"all":[[0,0],[0.5,0.9],[1,1]]},
                "subject":{"exposure":1}},"background":{"vignette":-50}}"#,
        )
        .unwrap();
        let subject = hostile.part(Part::Subject).unwrap();
        assert_eq!(subject.exposure, 5.0);
        assert_eq!(subject.grain, 0.0);
        assert!(subject.curves.is_identity() && !subject.has_parts());
        assert!(hostile.part(Part::Background).is_none());
    }

    fn blurred(kind: BlurKind) -> Recipe {
        let mut recipe = with(Adjustment::Blur, 100.0);
        recipe.blur_kind = kind;
        recipe
    }

    #[test]
    fn req_023_01_no_blur_or_blur_at_zero_changes_nothing() {
        let size = [4000.0, 3000.0];
        for kind in BlurKind::ALL {
            let mut recipe = blurred(kind);
            Adjustment::Blur.set(&mut recipe, 0.0);
            assert!(recipe.is_neutral(), "{kind:?}");
            assert_eq!(Params::new(&recipe).blur_sigma([0.1, 0.1], size, 0.0), 0.0);
        }
        // A strength without a kind changes nothing either.
        assert!(blurred(BlurKind::None).is_neutral());
        assert!(!blurred(BlurKind::Radial).is_neutral());
        // Strong blurs reach about 1.5 % of the longer side.
        let corner = Params::new(&blurred(BlurKind::Radial)).blur_sigma([0.0, 0.0], size, 0.0);
        assert!((corner - 60.0).abs() < 1.0, "{corner}");
        // Unknown kinds from newer versions read as none; the focus stays on the photo.
        let recipe = Recipe::from_json(r#"{"blurKind":"swirl","blur":50,"blurFocus":[3,-1]}"#).unwrap();
        assert_eq!(recipe.blur_kind, BlurKind::None);
        assert_eq!(recipe.blur_focus, [1.0, 0.0]);
        let round = blurred(BlurKind::TiltShift);
        assert_eq!(Recipe::from_json(&round.to_json()), Some(round));
    }

    #[test]
    fn req_023_02_background_blur_spares_the_subject() {
        let params = Params::new(&blurred(BlurKind::Background));
        let size = [1000.0, 1000.0];
        assert_eq!(params.blur_sigma(MID, size, 1.0), 0.0);
        let edge = params.blur_sigma(MID, size, 0.5);
        let background = params.blur_sigma(MID, size, 0.0);
        assert!(background > 10.0 && edge > 0.0 && edge < background);
    }

    #[test]
    fn req_023_03_tilt_shift_and_radial_keep_the_focus_sharp() {
        let size = [1000.0, 1000.0];
        let mut tilt = blurred(BlurKind::TiltShift);
        tilt.blur_focus = [0.5, 0.3];
        let tilt = Params::new(&tilt);
        // The band through the focus is sharp across the whole width, the rest grows blurrier.
        assert_eq!(tilt.blur_sigma([0.0, 0.3], size, 0.0), 0.0);
        assert_eq!(tilt.blur_sigma([1.0, 0.32], size, 0.0), 0.0);
        let (near, far) = (
            tilt.blur_sigma([0.5, 0.6], size, 0.0),
            tilt.blur_sigma([0.5, 1.0], size, 0.0),
        );
        assert!(near < far, "{near} < {far}");
        let radial = Params::new(&blurred(BlurKind::Radial));
        assert_eq!(radial.blur_sigma(MID, size, 0.0), 0.0);
        assert!(radial.blur_sigma([0.5, 0.2], size, 0.0) > 0.0);
        assert!(radial.blur_sigma([0.5, 0.2], size, 0.0) < radial.blur_sigma([0.0, 0.0], size, 0.0));
        // A larger focus keeps more sharp.
        let mut wide = blurred(BlurKind::Radial);
        Adjustment::BlurSize.set(&mut wide, 100.0);
        assert!(Params::new(&wide).blur_sigma([0.5, 0.2], size, 0.0) < radial.blur_sigma([0.5, 0.2], size, 0.0));
        assert_eq!(Recipe::default().blur_focus, [0.5, 0.5]);
    }

    /// REQ-023-05: a small light spreads into an evenly lit disc of the lens's radius, far brighter than a soft blur
    /// leaves it; flat areas stay as they are.
    #[test]
    fn req_023_05_a_light_spreads_into_an_even_disc() {
        const N: usize = 256;
        let background = 0.1;
        let pixels: Vec<[f32; 4]> = (0..N * N)
            .map(|i| {
                let light = (127..=129).contains(&(i % N)) && (127..=129).contains(&(i / N));
                if light {
                    [1.0; 4]
                } else {
                    [background, background, background, 1.0]
                }
            })
            .collect();
        let weighted = Pyramid::new(N as u32, N as u32, pixels.iter().copied().map(lens_weighted).collect());
        let soft = Pyramid::new(N as u32, N as u32, pixels);
        let radius = 40.0;
        let at = |dx: f32| [(128.5 + dx) / N as f32, 128.5 / N as f32];
        let lens = |dx: f32| lens_blurred(&weighted, at(dx), radius, [N as f32; 2]).unwrap()[0] - background;
        let inside: Vec<f32> = [0.0, 12.0, 24.0, 32.0].into_iter().map(lens).collect();
        let (low, high) = inside
            .iter()
            .fold((f32::MAX, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
        assert!(low > 0.02, "lit across the disc: {inside:?}");
        assert!(low / high > 0.5, "evenly: {inside:?}");
        assert!(lens(56.0).abs() < 0.003, "dark beyond its edge: {}", lens(56.0));
        let sigma = radius / LENS_RADIUS_PER_SIGMA;
        let soft_center = soft.blurred([background; 4], at(0.0), blur_lod(sigma, soft.levels()))[0] - background;
        assert!(low > 3.0 * soft_center, "{low} against a soft blur's {soft_center}");

        let flat = Pyramid::new(64, 64, vec![lens_weighted([0.3, 0.5, 0.2, 1.0]); 64 * 64]);
        let out = lens_blurred(&flat, [0.5, 0.5], 10.0, [64.0; 2]).unwrap();
        assert!(
            out.iter().zip([0.3, 0.5, 0.2]).all(|(a, b)| (a - b).abs() < 1e-4),
            "{out:?}"
        );
        assert_eq!(
            lens_blurred(&flat, [0.5, 0.5], 0.5, [64.0; 2]),
            None,
            "too small to blur"
        );
    }

    #[test]
    fn req_023_04_the_pyramid_blurs_smoothly_and_keeps_flat_areas() {
        // Halving down to at least 16 px, as the GPU's mip chain.
        let flat = Pyramid::new(64, 40, vec![[0.3, 0.5, 0.7, 1.0]; 64 * 40]);
        assert_eq!(flat.levels(), 2);
        let out = flat.blurred([0.0; 4], [0.2, 0.9], 1.0);
        assert!(
            out.iter().zip([0.3, 0.5, 0.7, 1.0]).all(|(a, b)| (a - b).abs() < 1e-5),
            "{out:?}"
        );
        // lod 0 is the sharp pixel itself.
        assert_eq!(flat.blurred([0.1, 0.2, 0.3, 0.4], MID, 0.0), [0.1, 0.2, 0.3, 0.4]);
        // An edge: blurrier with a higher lod, monotone across.
        let edge = Pyramid::new(
            128,
            128,
            (0..128 * 128).map(|i| [f32::from(i % 128 >= 64); 4]).collect(),
        );
        let at = |x: f32, lod: f32| edge.blurred([f32::from(x >= 0.5); 4], [x, 0.5], lod)[0];
        let row: Vec<f32> = (0..=40).map(|i| at(0.3 + i as f32 * 0.01, 2.0)).collect();
        assert!(row.windows(2).all(|w| w[1] >= w[0] - 1e-5), "{row:?}");
        assert!(
            at(0.47, 2.0) > 0.01 && at(0.47, 2.0) < at(0.47, 3.0),
            "{} {}",
            at(0.47, 2.0),
            at(0.47, 3.0)
        );
        // The lod for a blur measured in pixels of the pyramid's base.
        assert_eq!(blur_lod(0.0, 5), 0.0);
        assert!((blur_lod(BLUR_SIGMA_PER_LEVEL * 4.0, 5) - 2.0).abs() < 1e-5);
        assert_eq!(blur_lod(1e6, 5), 4.0);
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
    fn req_021_20_hue_turns_colors_keeping_lightness_and_chroma() {
        let lab = |rgb: [f32; 3]| {
            let [l, a, b] = to_oklab(rgb.map(to_linear));
            (l, (a * a + b * b).sqrt(), b.atan2(a).to_degrees())
        };
        // A muted color, so the turned one stays within sRGB.
        let color = [0.6, 0.45, 0.4];
        let (l, chroma, hue) = lab(color);
        for degrees in [-120.0, 30.0, 90.0] {
            let (l2, chroma2, hue2) = lab(render(&with(Adjustment::Hue, degrees), color));
            assert!((l2 - l).abs() < 2e-3, "{degrees}°: lightness {l} → {l2}");
            assert!(
                (chroma2 - chroma).abs() < 2e-3,
                "{degrees}°: chroma {chroma} → {chroma2}"
            );
            let turned = (hue2 - hue - degrees).rem_euclid(360.0);
            assert!(turned.min(360.0 - turned) < 1.0, "{degrees}°: hue {hue} → {hue2}");
        }
        // Greys stay grey; ±180° are the same; 0° changes nothing.
        let out = render(&with(Adjustment::Hue, 75.0), grey(0.5));
        assert!(close(out, grey(0.5)), "{out:?}");
        assert!(close(
            render(&with(Adjustment::Hue, 180.0), color),
            render(&with(Adjustment::Hue, -180.0), color)
        ));
        assert!(with(Adjustment::Hue, 0.0).is_neutral());
        assert!(!with(Adjustment::Hue, 1.0).is_neutral());
        assert_eq!(Adjustment::Hue.range(), (-180.0, 180.0));
        assert_eq!(Adjustment::Hue.section(), Section::Color);
        // After _Tint_ in the _Color_ section, and the subject and background have it too (REQ-022-01).
        let color_section: Vec<Adjustment> = Adjustment::ALL
            .into_iter()
            .filter(|a| a.section() == Section::Color)
            .collect();
        assert_eq!(
            color_section[..3],
            [Adjustment::Temperature, Adjustment::Tint, Adjustment::Hue]
        );
        assert!(Adjustment::Hue.in_parts());
    }

    #[test]
    fn req_021_20_the_hue_is_saved_with_the_recipe_and_older_recipes_have_none() {
        let mut recipe = with(Adjustment::Hue, -45.0);
        Adjustment::Hue.set(recipe.part_mut(Part::Subject), 120.0);
        let again = Recipe::from_json(&recipe.to_json()).unwrap();
        assert_eq!(again.hue, -45.0);
        assert_eq!(again.subject.as_ref().unwrap().hue, 120.0);
        assert_eq!(Recipe::from_json(r#"{"version": 1, "exposure": 1}"#).unwrap().hue, 0.0);
        assert_eq!(Recipe::from_json(r#"{"hue": 999}"#).unwrap().hue, 180.0);
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

    fn masked(shape: MaskShape, adjustment: Adjustment, value: f32) -> Recipe {
        let mut mask = LocalMask::new(shape);
        adjustment.set(&mut mask.recipe, value);
        Recipe {
            masks: vec![mask],
            ..Recipe::default()
        }
    }

    const GREY: [f32; 3] = [0.5, 0.5, 0.5];

    /// REQ-030-02: full where the mask selects, nothing where it doesn't, softly in between.
    #[test]
    fn req_030_02_a_mask_changes_only_what_it_selects() {
        let shape = MaskShape::Linear {
            from: [0.0, 0.0],
            to: [0.0, 1.0],
        };
        let params = Params::new(&masked(shape, Adjustment::Exposure, 1.0));
        let at = |v: f32| params.pixel(GREY, [0.5, v], SIZE)[1];
        let full = Params::new(&with(Adjustment::Exposure, 1.0)).pixel(GREY, MID, SIZE)[1];
        assert!((at(0.0) - full).abs() < 0.01, "{} vs {full}", at(0.0));
        assert!((at(1.0) - 0.5).abs() < 0.01, "{}", at(1.0));
        assert!(at(0.5) > 0.52 && at(0.5) < full - 0.02, "{}", at(0.5));
        // Neutral masks change nothing.
        let neutral = Recipe {
            masks: vec![LocalMask::new(MaskShape::LINEAR)],
            ..Recipe::default()
        };
        assert!(neutral.is_neutral());
        assert_eq!(
            Params::new(&neutral).pixel(GREY, MID, SIZE),
            Params::new(&Recipe::default()).pixel(GREY, MID, SIZE)
        );
    }

    /// REQ-030-02: each mask works on the result of the one before.
    #[test]
    fn req_030_02_masks_apply_in_order() {
        let all = MaskShape::Luminance {
            low: 0.0,
            high: 100.0,
            feather: 0.0,
        };
        let mut warmer = LocalMask::new(all);
        Adjustment::Temperature.set(&mut warmer.recipe, 60.0);
        let mut pale = LocalMask::new(all);
        Adjustment::Saturation.set(&mut pale.recipe, -100.0);
        let recipe = |masks: Vec<LocalMask>| Recipe {
            masks,
            ..Recipe::default()
        };
        let cast = |rgb: [f32; 3]| (rgb[0] - rgb[2]).abs();
        let pale_last = Params::new(&recipe(vec![warmer.clone(), pale.clone()])).pixel(GREY, MID, SIZE);
        assert!(cast(pale_last) < 0.01, "{pale_last:?}");
        let warm_last = Params::new(&recipe(vec![pale, warmer])).pixel(GREY, MID, SIZE);
        assert!(cast(warm_last) > 0.05, "{warm_last:?}");
    }

    /// REQ-030-03: full at the first handle, nothing at the second, whichever way it points.
    #[test]
    fn req_030_03_a_gradient_fades_between_its_handles() {
        let shape = MaskShape::Linear {
            from: [0.2, 0.5],
            to: [0.8, 0.5],
        };
        let weight = |u: f32| shape.weight(GREY, [u, 0.5], SIZE);
        assert_eq!(weight(0.1), 1.0);
        assert_eq!(weight(0.2), 1.0);
        assert_eq!(weight(0.8), 0.0);
        assert_eq!(weight(0.9), 0.0);
        assert!((weight(0.5) - 0.5).abs() < 1e-5);
        assert!(weight(0.35) > weight(0.5) && weight(0.5) > weight(0.65));
        let same = MaskShape::Linear {
            from: [0.5, 0.5],
            to: [0.5, 0.5],
        };
        assert_eq!(same.weight(GREY, [0.1, 0.9], SIZE), 0.0, "no direction selects nothing");
    }

    /// REQ-030-04: what is that bright as stored, whatever the photo's own sliders do.
    #[test]
    fn req_030_04_a_luminance_mask_reads_the_unedited_photo() {
        let shape = MaskShape::Luminance {
            low: 40.0,
            high: 60.0,
            feather: 10.0,
        };
        let weight = |v: f32| shape.weight([v; 3], MID, SIZE);
        assert_eq!(weight(0.5), 1.0);
        assert_eq!(weight(0.2), 0.0);
        assert_eq!(weight(0.8), 0.0);
        assert!(weight(0.35) > 0.0 && weight(0.35) < 1.0, "{}", weight(0.35));

        let mut recipe = masked(shape, Adjustment::Exposure, 1.0);
        assert!(Params::new(&recipe).pixel(GREY, MID, SIZE)[1] > 0.6);
        // The photo twice as bright: the mask still selects this pixel, and adds to it.
        Adjustment::Exposure.set(&mut recipe, 1.0);
        let photo = Params::new(&with(Adjustment::Exposure, 1.0)).pixel(GREY, MID, SIZE)[1];
        assert!(Params::new(&recipe).pixel(GREY, MID, SIZE)[1] > photo + 0.1);
    }

    /// REQ-030-05: the chosen hues, and no greys.
    #[test]
    fn req_030_05_a_color_mask_selects_hues_and_no_greys() {
        let weight = |shape: MaskShape, rgb: [f32; 3]| shape.weight(rgb, MID, SIZE);
        let greens = MaskShape::COLOR;
        let (green, red) = ([0.2, 0.6, 0.2], [0.7, 0.15, 0.15]);
        assert_eq!(weight(greens, green), 1.0);
        assert_eq!(weight(greens, red), 0.0);
        assert_eq!(weight(greens, GREY), 0.0);
        // Across the wheel's seam: red lies just past 0°, the mask's center just before it.
        let reds = MaskShape::Color {
            hue: 355.0,
            range: 40.0,
            feather: 10.0,
        };
        assert_eq!(weight(reds, red), 1.0);
        assert_eq!(weight(reds, green), 0.0);
    }

    /// REQ-030-06: _Invert_ selects what was left out.
    #[test]
    fn req_030_06_invert_swaps_the_selection() {
        let mut recipe = masked(MaskShape::COLOR, Adjustment::Exposure, 1.0);
        let green = [0.2, 0.6, 0.2];
        let plain = Params::new(&Recipe::default());
        assert_ne!(
            Params::new(&recipe).pixel(green, MID, SIZE),
            plain.pixel(green, MID, SIZE)
        );
        assert_eq!(
            Params::new(&recipe).pixel(GREY, MID, SIZE),
            plain.pixel(GREY, MID, SIZE)
        );
        recipe.masks[0].invert = true;
        assert_eq!(
            Params::new(&recipe).pixel(green, MID, SIZE),
            plain.pixel(green, MID, SIZE)
        );
        assert_ne!(
            Params::new(&recipe).pixel(GREY, MID, SIZE),
            plain.pixel(GREY, MID, SIZE)
        );
    }

    /// REQ-030-14: paintings survive saving, and one no shape uses any more goes, with the others' places following.
    #[test]
    fn req_030_14_paintings_are_saved_and_tidied() {
        use crate::paint::{Painting, Stroke};
        let painting = |x: f32| Painting {
            strokes: vec![Stroke {
                points: vec![[x, 0.5]],
                size: 5.0,
                softness: 50.0,
                erase: false,
                closed: false,
            }],
        };
        let painted = |layer: u8| LocalMask::new(MaskShape::Painted { layer, freeform: false });
        let mut recipe = Recipe {
            paintings: vec![painting(0.1), painting(0.2)],
            masks: vec![painted(0), painted(1)],
            ..Recipe::default()
        };
        assert_eq!(Recipe::from_json(&recipe.to_json()), Some(recipe.clone()));
        assert!(!Recipe::default().to_json().contains("paintings"));

        recipe.masks.remove(0);
        recipe.tidy();
        assert_eq!(recipe.paintings, vec![painting(0.2)]);
        assert_eq!(
            recipe.masks[0].shape,
            MaskShape::Painted {
                layer: 0,
                freeform: false
            }
        );
    }

    /// REQ-030-12: a painted shape selects what its grid says, and nothing without one or beyond them.
    #[test]
    fn req_030_12_a_painted_shape_reads_its_painting() {
        let shape = |layer| MaskShape::Painted { layer, freeform: false };
        let recipe = |layer| Recipe {
            masks: vec![LocalMask::new(shape(layer))],
            ..Recipe::default()
        };
        let weight = |params: &Params| params.masks[0].weight(GREY, MID, SIZE);
        assert_eq!(weight(&Params::new(&recipe(0))), 0.0, "not drawn");
        let grid = Arc::new(Mask::new(2, 2, vec![0.25; 4]).unwrap());
        assert_eq!(
            weight(&Params::new(&recipe(0)).with_paintings(std::slice::from_ref(&grid))),
            0.25
        );
        assert_eq!(
            weight(&Params::new(&recipe(3)).with_paintings(&[grid])),
            0.0,
            "no such painting"
        );
    }

    /// REQ-030-08: masks survive saving; values from a file stay in range; an unknown kind is left out, not the recipe.
    #[test]
    fn req_030_08_shapes_from_a_development_build_are_left_out() {
        let json = r#"{"version":1,"masks":[
            {"shape":{"kind":"subject"}},
            {"shape":{"kind":"linear","from":[0,0],"to":[0,1]},
             "more":[{"op":"subtract","shape":{"kind":"subject"}}]}
        ]}"#;
        let read = Recipe::from_json(json).unwrap();
        assert_eq!(read.masks.len(), 1, "the subject is no shape any more");
        assert!(matches!(read.masks[0].shape, MaskShape::Linear { .. }));
        assert!(!read.needs_subject());
    }

    /// REQ-030-08: masks survive saving, values come into their ranges.
    #[test]
    fn req_030_08_masks_are_saved_and_values_stay_in_range() {
        let mut recipe = masked(MaskShape::LINEAR, Adjustment::Exposure, 0.5);
        recipe.masks.push(LocalMask {
            shape: MaskShape::LUMINANCE,
            invert: true,
            recipe: Box::default(),
        });
        assert_eq!(Recipe::from_json(&recipe.to_json()), Some(recipe));
        assert!(
            !Recipe::default().to_json().contains("masks"),
            "recipes without masks stay as they were"
        );

        let hostile = r#"{"version":1,"masks":[
            {"shape":{"kind":"luminance","low":-50,"high":900,"feather":1e30},"recipe":{"exposure":99,"grain":50}},
            {"shape":{"kind":"spiral","turns":3}},
            {"shape":{"kind":"linear","from":[50,-50],"to":[0,0]}},
            {"shape":{"kind":"color","hue":720,"range":-1,"feather":500}}]}"#;
        let read = Recipe::from_json(hostile).unwrap();
        assert_eq!(read.masks.len(), 3, "{read:?}");
        assert_eq!(
            read.masks[0].shape,
            MaskShape::Luminance {
                low: 0.0,
                high: 100.0,
                feather: 100.0
            }
        );
        let (low, high) = Adjustment::Exposure.range();
        assert!((low..=high).contains(&read.masks[0].recipe.exposure));
        assert_eq!(read.masks[0].recipe.grain, 0.0, "only light and color");
        assert_eq!(
            read.masks[1].shape,
            MaskShape::Linear {
                from: [2.0, -1.0],
                to: [0.0, 0.0]
            }
        );
        let dangling = r#"{"version":1,"masks":[{"shape":{"kind":"painted","layer":9,"freeform":false}}]}"#;
        assert!(
            Recipe::from_json(dangling).unwrap().masks.is_empty(),
            "no such painting"
        );
        let many = format!(
            r#"{{"version":1,"masks":[{}]}}"#,
            vec![r#"{"shape":{"kind":"luminance","low":0,"high":100,"feather":0}}"#; 20].join(",")
        );
        assert_eq!(Recipe::from_json(&many).unwrap().masks.len(), MAX_MASKS);
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

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    /// REQ-029-01, REQ-029-06: the output's size and where points land, turned clockwise.
    #[test]
    fn req_029_01_quarter_turns_swap_the_sides_and_move_points_clockwise() {
        for (turns, size, corner) in [
            (0, (40, 20), [0.0, 0.0]),
            (1, (20, 40), [1.0, 0.0]),
            (2, (40, 20), [1.0, 1.0]),
            (3, (20, 40), [0.0, 1.0]),
        ] {
            let geometry = Geometry::new(None, turns);
            assert_eq!(geometry.output_size(40, 20), size, "{turns}");
            assert!(near(geometry.to_output([0.0, 0.0]), corner), "{turns}");
        }
        assert!(!Geometry::new(None, 1).is_identity());
        assert!(Geometry::new(None, 0).is_identity());
    }

    /// REQ-029-02: the frame on screen and the crop on the content are each other's images, for every turn.
    #[test]
    fn req_029_02_the_frame_on_screen_and_the_crop_agree() {
        let crop = [0.2, 0.1, 0.5, 0.4];
        for turns in 0..4 {
            let frame = Geometry::new(Some(crop), turns).frame();
            let back = Geometry::crop_of_frame(frame, turns).unwrap();
            for (a, b) in back.iter().zip(crop) {
                assert!((a - b).abs() < 1e-5, "{turns}: {back:?}");
            }
        }
        // Turned clockwise, the photo's left edge is on top.
        let frame = Geometry::new(Some([0.0, 0.0, 0.5, 1.0]), 1).frame();
        assert!(
            near([frame[0], frame[1]], [0.0, 0.0]) && near([frame[2], frame[3]], [1.0, 0.5]),
            "{frame:?}"
        );
        assert_eq!(Geometry::crop_of_frame([0.0, 0.0, 1.0, 1.0], 1), None);
    }

    /// REQ-029-07: editing a saved crop again works out the whole photo's size from the saved file's.
    #[test]
    fn req_029_07_the_originals_size_follows_from_the_outputs() {
        let geometry = Geometry::new(Some([0.25, 0.5, 0.5, 0.5]), 1);
        let (width, height) = geometry.output_size(40, 20);
        assert_eq!(geometry.original_size(width, height), (40, 20));
    }

    /// REQ-029-06: a crop in pixels, inside the photo and never empty, whatever the rounding.
    #[test]
    fn req_029_06_the_crop_in_pixels_stays_inside_the_photo() {
        let crop = Geometry::new(Some([0.25, 0.5, 0.5, 0.5]), 0);
        assert_eq!(crop.crop_pixels(40, 20), [10, 10, 20, 10]);
        assert_eq!(crop.output_size(40, 20), (20, 10));
        assert_eq!(
            Geometry::new(Some([0.25, 0.5, 0.5, 0.5]), 1).output_size(40, 20),
            (10, 20)
        );
        let tiny = Geometry::new(Some([0.999, 0.999, 0.0001, 0.0001]), 0);
        let [x, y, w, h] = tiny.crop_pixels(40, 20);
        assert!(w >= 1 && h >= 1 && x + w <= 40 && y + h <= 20, "{x} {y} {w} {h}");
    }

    /// REQ-029-04: a point of the photo and of the output are each other's images, for every crop and turn.
    #[test]
    fn req_029_04_the_mapping_to_the_output_and_back_agree() {
        for turns in 0..4 {
            let geometry = Geometry::new(Some([0.2, 0.1, 0.5, 0.4]), turns);
            for point in [[0.2, 0.1], [0.7, 0.5], [0.3, 0.3]] {
                assert!(
                    near(geometry.to_original(geometry.to_output(point)), point),
                    "{turns} {point:?}"
                );
            }
        }
        let crop = Geometry::new(Some([0.5, 0.5, 0.5, 0.5]), 0);
        assert!(near(crop.to_output([0.75, 0.75]), [0.5, 0.5]));
    }

    /// REQ-029-10 (#248): straightening keeps a spot of the photo on the same spot and back again, the canvas holds the
    /// turned photo, and the largest crop of a shape shows only the photo while a larger one wouldn't.
    #[test]
    fn req_029_10_straightening_maps_both_ways_and_the_crop_shows_only_the_photo() {
        let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        for degrees in [-100.0, -33.3, -0.1, 7.5, 45.0, 90.0, 100.0] {
            for turns in 0..4 {
                let whole = Geometry::new(None, turns).with_straighten(degrees, 1.5);
                let crop = whole.largest_crop(4.0 / 3.0);
                let geometry = Geometry::new(Some(crop), turns).with_straighten(degrees, 1.5);
                for point in [[0.3, 0.4], [0.5, 0.5], [0.7, 0.2]] {
                    assert!(
                        close(geometry.to_original(geometry.to_output(point)), point),
                        "{degrees}° {turns}"
                    );
                }
                assert!(
                    geometry.frame_shows_photo(geometry.frame()),
                    "{degrees}°: only the photo"
                );
                let [x, y, w, h] = geometry.frame();
                let larger = [x - 0.01, y - 0.01, w + 0.02, h + 0.02];
                assert!(!whole.frame_shows_photo(larger), "{degrees}°: the largest");
                assert!((geometry.crop_ratio() - 4.0 / 3.0).abs() < 1e-3, "keeps the shape");
            }
        }
        // A quarter of a turn makes the canvas the photo on its side; none at all leaves it as it was.
        let side = Geometry::new(None, 0).with_straighten(90.0, 1.5);
        assert_eq!(side.canvas_size(300, 200), (200, 300));
        assert_eq!(
            Geometry::new(None, 0).with_straighten(0.0, 1.5).canvas_size(300, 200),
            (300, 200)
        );
        let tilted = Geometry::new(None, 0).with_straighten(10.0, 1.5);
        assert!(
            tilted.shows_photo([0.5, 0.5]) && !tilted.shows_photo([0.0, 0.0]),
            "empty corners"
        );
        // A straightened, cropped output tells its original's size.
        let saved = Geometry::new(Some(tilted.largest_crop(1.5)), 1).with_straighten(10.0, 1.5);
        let (width, height) = saved.output_size(3000, 2000);
        let (w, h) = saved.original_size(width, height);
        assert!(w.abs_diff(3000) <= 2 && h.abs_diff(2000) <= 2, "{w} × {h}");
    }

    /// REQ-029-10: setting the straightening fits the crop to its shape inside the photo; it is kept in tenths of a
    /// degree within ±100°, round-trips, and is written as a version 2 recipe only when used.
    #[test]
    fn req_029_10_straightening_fits_the_crop_and_round_trips() {
        let mut recipe = Recipe::default();
        assert!(recipe.to_json().contains("\"version\":1"));
        recipe.set_straighten(12.34, 1.5);
        assert_eq!(recipe.straighten, 12.3);
        assert_eq!(recipe.aspect, Some(1.5));
        let geometry = recipe.geometry();
        assert!(geometry.frame_shows_photo(geometry.frame()));
        assert!((geometry.crop_ratio() - 1.5).abs() < 1e-3, "the photo's own shape");
        let json = recipe.to_json();
        assert!(json.contains("\"version\":2"), "{json}");
        assert_eq!(Recipe::from_json(&json), Some(recipe.clone()));
        recipe.set_straighten(0.0, 1.5);
        assert_eq!((recipe.crop, recipe.aspect), (None, None), "back to the whole photo");
        recipe.set_straighten(250.0, 1.5);
        assert_eq!(recipe.straighten, MAX_STRAIGHTEN);
        let hostile = r#"{"version":2,"straighten":"NaN","aspect":-3}"#;
        assert!(Recipe::from_json(hostile).is_none_or(|recipe| recipe.straighten == 0.0));
        let tamed = Recipe::from_json(r#"{"version":2,"straighten":-400,"aspect":1e9}"#).unwrap();
        assert_eq!((tamed.straighten, tamed.aspect), (-MAX_STRAIGHTEN, Some(100.0)));
        assert!(Recipe::from_json(r#"{"version":3}"#).is_none(), "a newer app's");
    }

    /// REQ-029-11 (#248): flipping mirrors what shows, crop, straightening and quarter turns included, and twice is
    /// as before.
    #[test]
    fn req_029_11_flips_mirror_the_output() {
        let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        for turns in 0..4 {
            for horizontal in [true, false] {
                let mut recipe = Recipe {
                    quarter_turns: turns,
                    ..Recipe::default()
                };
                recipe.set_straighten(8.0, 1.5);
                recipe.crop = Some([0.2, 0.25, 0.5, 0.4]);
                let before = recipe.geometry();
                let mut flipped = recipe.clone();
                flipped.flip(horizontal);
                let after = flipped.geometry();
                for [u, v] in [[0.1, 0.2], [0.5, 0.5], [0.8, 0.6]] {
                    let mirrored = if horizontal { [1.0 - u, v] } else { [u, 1.0 - v] };
                    assert!(
                        close(after.to_original(mirrored), before.to_original([u, v])),
                        "{turns} {horizontal}"
                    );
                }
                flipped.flip(horizontal);
                assert_eq!(flipped.straighten, recipe.straighten);
                assert_eq!((flipped.flip_h, flipped.flip_v), (false, false));
                assert!(
                    flipped
                        .crop
                        .unwrap()
                        .iter()
                        .zip(recipe.crop.unwrap())
                        .all(|(a, b)| (a - b).abs() < 1e-6)
                );
            }
        }
        let mut flipped = Recipe::default();
        flipped.flip(true);
        assert!(!flipped.is_neutral() && flipped.to_json().contains("\"version\":2"));
    }

    /// REQ-029-08, REQ-029-10, REQ-029-11: a region follows flips and straightening: mirrored, and as the box around
    /// it once turned, cut at the crop's edge.
    #[test]
    fn req_029_08_regions_follow_flips_and_straightening() {
        let flipped = Geometry::new(None, 0).with_flips(true, false);
        let placed = flipped.place([0.1, 0.2, 0.2, 0.3]).unwrap();
        assert!(
            placed
                .iter()
                .zip([0.7, 0.2, 0.2, 0.3])
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "{placed:?}"
        );

        let mut recipe = Recipe::default();
        recipe.set_straighten(20.0, 1.5);
        let geometry = recipe.geometry();
        let face = [0.45, 0.45, 0.1, 0.1];
        let [x, y, w, h] = geometry.place(face).unwrap();
        let center = geometry.to_output([0.5, 0.5]);
        assert!((x + w / 2.0 - center[0]).abs() < 1e-3 && (y + h / 2.0 - center[1]).abs() < 1e-3);
        assert!(
            w > face[2] && h > face[3],
            "the box around the turned face is larger: {w} {h}"
        );
        assert_eq!(
            geometry.place([0.0, 0.0, 0.02, 0.02]),
            None,
            "a corner the crop leaves out"
        );
    }

    /// REQ-029-08: regions follow the crop and the turn, are cut at its edge, and are gone when entirely outside.
    #[test]
    fn req_029_08_places_rectangles_on_the_output() {
        let crop = Geometry::new(Some([0.5, 0.0, 0.5, 1.0]), 0);
        let inside = crop.place([0.6, 0.2, 0.2, 0.2]).unwrap();
        assert!(near([inside[0], inside[1]], [0.2, 0.2]) && near([inside[2], inside[3]], [0.4, 0.2]));
        let cut = crop.place([0.4, 0.2, 0.2, 0.2]).unwrap();
        assert!(near([cut[0], cut[2]], [0.0, 0.2]), "{cut:?}");
        assert_eq!(crop.place([0.1, 0.2, 0.2, 0.2]), None);
        // A quarter turn clockwise: the left edge becomes the top.
        let turned = Geometry::new(None, 1).place([0.0, 0.0, 0.2, 0.4]).unwrap();
        assert!(
            near([turned[0], turned[1]], [0.6, 0.0]) && near([turned[2], turned[3]], [0.4, 0.2]),
            "{turned:?}"
        );
    }

    /// REQ-029-07: both survive the file, older recipes have neither, and hostile values are tamed (REQ-011-10).
    #[test]
    fn req_029_07_crop_and_turns_round_trip_and_hostile_ones_are_tamed() {
        let recipe = Recipe {
            quarter_turns: 3,
            crop: Some([0.25, 0.25, 0.5, 0.5]),
            ..Default::default()
        };
        assert!(!recipe.is_neutral());
        assert_eq!(Recipe::from_json(&recipe.to_json()), Some(recipe));
        let older = Recipe::from_json(&Recipe::default().to_json()).unwrap();
        assert_eq!((older.quarter_turns, older.crop), (0, None));
        assert!(!Recipe::default().to_json().contains("crop"));

        let parse = |json: &str| Recipe::from_json(json).unwrap();
        assert_eq!(parse(r#"{"quarterTurns":7}"#).quarter_turns, 3);
        for bad in [
            r#"{"crop":[0.5,0.5,0,0.5]}"#,
            r#"{"crop":[0.5,0.5,-1,0.5]}"#,
            r#"{"crop":[2,0,0.5,0.5]}"#,
            r#"{"crop":[0,0,1,1]}"#,
        ] {
            assert_eq!(parse(bad).crop, None, "{bad}");
        }
        assert_eq!(parse(r#"{"crop":[0.5,0.5,9,9]}"#).crop, Some([0.5, 0.5, 0.5, 0.5]));
    }

    /// REQ-029-04: the vignette's center and edges are the crop's; only pixels away from it darken.
    #[test]
    fn req_029_04_the_vignette_follows_the_crop() {
        let recipe = Recipe {
            vignette: -100.0,
            crop: Some([0.5, 0.5, 0.5, 0.5]),
            ..Default::default()
        };
        let params = Params::new(&recipe);
        let gray = [0.5, 0.5, 0.5];
        let at = |uv: [f32; 2]| params.pixel(gray, uv, [100.0, 100.0])[0];
        assert!((at([0.75, 0.75]) - 0.5).abs() < 0.01, "the crop's center stays");
        assert!(at([0.99, 0.99]) < 0.3, "the crop's corner darkens");
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
