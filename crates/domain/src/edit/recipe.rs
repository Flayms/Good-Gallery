//! An edit's [`Recipe`], its defaults and its size and quality on saving.

use super::*;

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
    /// A LUT, first of all (REQ-043-02).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lut: Option<crate::lut::LutChoice>,
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

pub(super) fn is_zero(turns: &u8) -> bool {
    *turns == 0
}

pub(super) fn is_zero_f32(value: &f32) -> bool {
    *value == 0.0
}

pub(super) fn is_false(value: &bool) -> bool {
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
            lut: None,
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
            && self.lut.is_none()
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
        if let Some(lut) = &mut self.lut {
            lut.sanitize();
        }
        self.quarter_turns %= 4;
        self.crop = self.crop.and_then(sanitized_crop);
        self.straighten = sanitized_straighten(self.straighten);
        self.aspect = (self.straighten != 0.0).then(|| sanitized_aspect(self.aspect.unwrap_or(1.0)));
    }
}
