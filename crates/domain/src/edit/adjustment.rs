//! The adjustments panel's sections and sliders.

use super::*;

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
