//! Film looks (spec 028): the color response, contrast, grain and vignette of a film or instant camera, made of the
//! editor's own adjustments and curves, so the preview, the CPU path and saving render them without a stage of their
//! own. [`fold`] blends the chosen look into a recipe at its strength.

use crate::edit::{Adjustment, Curve, Curves, Recipe};
use serde::{Deserialize, Serialize};

/// The points of the RGB, red, green and blue curves.
type CurvePoints = [&'static [[f32; 2]]; 4];

/// A film look. Tuned by eye against reference scans: the character of each film, not a measured response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Look {
    Polaroid600,
    PolaroidBw,
    Portra400,
    Velvia50,
    Gold200,
    Hp5,
}

impl Look {
    pub const ALL: [Look; 6] = [
        Self::Polaroid600,
        Self::PolaroidBw,
        Self::Portra400,
        Self::Velvia50,
        Self::Gold200,
        Self::Hp5,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Polaroid600 => "Polaroid 600",
            Self::PolaroidBw => "Polaroid B&W",
            Self::Portra400 => "Portra 400",
            Self::Velvia50 => "Velvia 50",
            Self::Gold200 => "Gold 200",
            Self::Hp5 => "HP5",
        }
    }

    /// The look at full strength, as the editor's adjustments and curves.
    pub fn recipe(self) -> Recipe {
        use Adjustment as A;
        let (adjustments, curves): (&[(Adjustment, f32)], CurvePoints) = match self {
            // Soft, faded and warm in the highlights, cyan in the shadows, with fine grain and dark corners.
            Self::Polaroid600 => (
                &[
                    (A::Exposure, 0.1),
                    (A::Contrast, -20.0),
                    (A::Highlights, -25.0),
                    (A::Shadows, 10.0),
                    (A::Whites, -15.0),
                    (A::Temperature, 12.0),
                    (A::Tint, 4.0),
                    (A::Saturation, -15.0),
                    (A::Vibrance, -10.0),
                    (A::Grain, 30.0),
                    (A::GrainSize, 45.0),
                    (A::Vignette, -20.0),
                    (A::VignetteMidpoint, 40.0),
                ],
                [
                    &[[0.0, 0.07], [0.5, 0.52], [1.0, 0.95]],
                    &[[0.0, 0.0], [0.75, 0.79], [1.0, 1.0]],
                    &[[0.0, 0.03], [1.0, 1.0]],
                    &[[0.0, 0.06], [0.5, 0.5], [1.0, 0.94]],
                ],
            ),
            // Low-contrast black and white with a cool cast and lifted blacks.
            Self::PolaroidBw => (
                &[
                    (A::Saturation, -100.0),
                    (A::Contrast, -10.0),
                    (A::Highlights, -15.0),
                    (A::Grain, 35.0),
                    (A::GrainSize, 40.0),
                    (A::Vignette, -15.0),
                    (A::VignetteMidpoint, 40.0),
                ],
                [
                    &[[0.0, 0.06], [1.0, 0.94]],
                    &[[0.0, 0.0], [1.0, 0.98]],
                    &[[0.0, 0.0], [1.0, 1.0]],
                    &[[0.0, 0.02], [1.0, 1.0]],
                ],
            ),
            // Warm, soft portrait film: gentle contrast, kind to skin, muted greens.
            Self::Portra400 => (
                &[
                    (A::Exposure, 0.05),
                    (A::Contrast, -10.0),
                    (A::Highlights, -15.0),
                    (A::Shadows, 10.0),
                    (A::Temperature, 8.0),
                    (A::Saturation, -8.0),
                    (A::Vibrance, 10.0),
                    (A::Grain, 15.0),
                    (A::GrainSize, 30.0),
                ],
                [
                    &[[0.0, 0.03], [0.25, 0.24], [0.75, 0.78], [1.0, 0.98]],
                    &[[0.0, 0.0], [0.5, 0.52], [1.0, 1.0]],
                    &[[0.0, 0.0], [0.5, 0.49], [1.0, 0.98]],
                    &[[0.0, 0.0], [1.0, 1.0]],
                ],
            ),
            // Slide film: deep blacks, strong contrast, saturated blues and greens, almost no grain.
            Self::Velvia50 => (
                &[
                    (A::Contrast, 25.0),
                    (A::Blacks, -10.0),
                    (A::Highlights, -10.0),
                    (A::Temperature, -3.0),
                    (A::Tint, -3.0),
                    (A::Saturation, 30.0),
                    (A::Vibrance, 25.0),
                    (A::Grain, 8.0),
                    (A::GrainSize, 20.0),
                    (A::Vignette, -10.0),
                ],
                [
                    &[[0.0, 0.0], [0.25, 0.21], [0.75, 0.8], [1.0, 1.0]],
                    &[[0.0, 0.0], [1.0, 1.0]],
                    &[[0.0, 0.0], [0.5, 0.52], [1.0, 1.0]],
                    &[[0.0, 0.0], [0.4, 0.43], [1.0, 1.0]],
                ],
            ),
            // Consumer film: warm and yellow, punchy midtones, visible grain.
            Self::Gold200 => (
                &[
                    (A::Contrast, 12.0),
                    (A::Temperature, 15.0),
                    (A::Tint, -3.0),
                    (A::Saturation, 12.0),
                    (A::Vibrance, 5.0),
                    (A::Grain, 30.0),
                    (A::GrainSize, 45.0),
                ],
                [
                    &[[0.0, 0.04], [0.5, 0.53], [1.0, 0.97]],
                    &[[0.0, 0.0], [0.5, 0.53], [1.0, 1.0]],
                    &[[0.0, 0.0], [1.0, 1.0]],
                    &[[0.0, 0.0], [0.5, 0.45], [1.0, 0.92]],
                ],
            ),
            // Punchy black and white with coarse grain.
            Self::Hp5 => (
                &[
                    (A::Saturation, -100.0),
                    (A::Contrast, 30.0),
                    (A::Blacks, -15.0),
                    (A::Whites, 10.0),
                    (A::Grain, 45.0),
                    (A::GrainSize, 60.0),
                    (A::Vignette, -10.0),
                ],
                [
                    &[[0.0, 0.02], [0.25, 0.2], [0.75, 0.8], [1.0, 1.0]],
                    &[[0.0, 0.0], [1.0, 1.0]],
                    &[[0.0, 0.0], [1.0, 1.0]],
                    &[[0.0, 0.0], [1.0, 1.0]],
                ],
            ),
        };
        let mut recipe = Recipe::default();
        for &(adjustment, value) in adjustments {
            adjustment.set(&mut recipe, value);
        }
        let curve = |points: &[[f32; 2]]| Curve {
            points: points.to_vec(),
        };
        recipe.curves = Curves {
            all: curve(curves[0]),
            red: curve(curves[1]),
            green: curve(curves[2]),
            blue: curve(curves[3]),
        };
        recipe
    }
}

/// The look an edit uses, how strongly, and how much of its grain (REQ-028-01 … REQ-028-03).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookChoice {
    pub look: Look,
    /// 0 to 100 %.
    pub strength: f32,
    /// The look's grain, 0 to 200 %.
    pub grain: f32,
}

impl LookChoice {
    pub const STRENGTH: (f32, f32) = (0.0, 100.0);
    pub const GRAIN: (f32, f32) = (0.0, 200.0);

    /// `look` at full strength with its own grain.
    pub fn new(look: Look) -> Self {
        Self {
            look,
            strength: 100.0,
            grain: 100.0,
        }
    }

    /// Values from a file brought into their ranges (REQ-011-10).
    pub fn sanitize(&mut self) {
        let fit = |value: f32, (low, high): (f32, f32)| {
            if value.is_finite() {
                value.clamp(low, high)
            } else {
                100.0
            }
        };
        self.strength = fit(self.strength, Self::STRENGTH);
        self.grain = fit(self.grain, Self::GRAIN);
    }
}

/// `recipe` with its look blended in (REQ-028-02 … REQ-028-04), and the look's curves at its strength, which go before
/// the recipe's own. Without a look, the recipe as it is and no curves.
pub fn fold(recipe: &Recipe) -> (Recipe, Option<Curves>) {
    let Some(choice) = recipe.look else {
        return (recipe.clone(), None);
    };
    let look = choice.look.recipe();
    let strength = choice.strength / 100.0;
    let mut folded = recipe.clone();
    folded.look = None;
    for adjustment in Adjustment::ALL {
        let (low, high) = adjustment.range();
        let own = adjustment.get(recipe);
        let value = match adjustment {
            // Grain follows its own slider, not the strength (REQ-028-03).
            Adjustment::Grain => own + adjustment.get(&look) * choice.grain / 100.0,
            // Where the user set none, the look's grain size and vignette shape.
            Adjustment::GrainSize if recipe.grain == 0.0 => adjustment.get(&look),
            Adjustment::VignetteMidpoint if recipe.vignette == 0.0 => adjustment.get(&look),
            Adjustment::GrainSize | Adjustment::VignetteMidpoint => own,
            _ => own + (adjustment.get(&look) - adjustment.neutral()) * strength,
        };
        adjustment.set(&mut folded, value.clamp(low, high));
    }
    let curves = (strength > 0.0 && !look.curves.is_identity()).then(|| {
        let blend = |curve: &Curve| Curve {
            points: curve.points.iter().map(|&[x, y]| [x, x + (y - x) * strength]).collect(),
        };
        Curves {
            all: blend(&look.curves.all),
            red: blend(&look.curves.red),
            green: blend(&look.curves.green),
            blue: blend(&look.curves.blue),
        }
    });
    (folded, curves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Params;

    const SIZE: [f32; 2] = [1000.0, 1000.0];
    /// Colors across the range: dark, skin, sky, foliage, light.
    const COLORS: [[f32; 3]; 5] = [
        [0.1, 0.08, 0.06],
        [0.85, 0.65, 0.55],
        [0.35, 0.55, 0.85],
        [0.25, 0.45, 0.15],
        [0.9, 0.9, 0.88],
    ];

    fn with_look(look: Look, strength: f32, grain: f32) -> Recipe {
        Recipe {
            look: Some(LookChoice { look, strength, grain }),
            ..Recipe::default()
        }
    }

    /// The pixel at the photo's center, where no vignette reaches.
    fn develop(recipe: &Recipe, rgb: [f32; 3]) -> [f32; 3] {
        Params::new(recipe).pixel(rgb, [0.5, 0.5], SIZE)
    }

    fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
        (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn req_028_01_every_look_changes_the_photo_and_black_and_white_ones_drop_color() {
        for look in Look::ALL {
            let recipe = with_look(look, 100.0, 0.0);
            assert!(!recipe.is_neutral(), "{look:?}");
            for rgb in COLORS {
                let out = develop(&recipe, rgb);
                assert!(
                    distance(out, develop(&Recipe::default(), rgb)) > 0.01,
                    "{look:?} leaves {rgb:?} as it is"
                );
                if matches!(look, Look::PolaroidBw | Look::Hp5) {
                    // Grey, give or take the cool tint of the instant film.
                    assert!(
                        distance(out, [out[1]; 3]) < 0.03,
                        "{look:?} keeps the color of {rgb:?}: {out:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn req_028_02_strength_moves_between_none_and_the_full_look() {
        for look in Look::ALL {
            for rgb in COLORS {
                let none = develop(&with_look(look, 0.0, 0.0), rgb);
                assert!(
                    distance(none, develop(&Recipe::default(), rgb)) < 1e-6,
                    "{look:?} at 0 %"
                );
                let half = develop(&with_look(look, 50.0, 0.0), rgb);
                let full = develop(&with_look(look, 100.0, 0.0), rgb);
                for c in 0..3 {
                    let (low, high) = (none[c].min(full[c]), none[c].max(full[c]));
                    assert!(
                        (low - 0.01..=high + 0.01).contains(&half[c]),
                        "{look:?} at 50 % isn't between none and full for {rgb:?}: {none:?} {half:?} {full:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn req_028_03_grain_changes_only_the_grain() {
        let look = Look::Gold200;
        let own = look.recipe().grain / 100.0;
        let grain = |percent: f32| Params::new(&with_look(look, 100.0, percent));
        assert_eq!(grain(0.0).grain, 0.0);
        assert!((grain(100.0).grain - own).abs() < 1e-6);
        assert!((grain(200.0).grain - 2.0 * own).abs() < 1e-6);
        // Without its grain, the colors are those of the look at any grain.
        let mut doubled = grain(200.0);
        doubled.grain = 0.0;
        for rgb in COLORS {
            assert_eq!(
                doubled.pixel(rgb, [0.5, 0.5], SIZE),
                grain(0.0).pixel(rgb, [0.5, 0.5], SIZE)
            );
        }
    }

    #[test]
    fn req_028_04_adjustments_add_to_the_look_and_curves_bend_its_result() {
        let mut recipe = with_look(Look::Velvia50, 100.0, 100.0);
        recipe.contrast = 10.0;
        recipe.grain = 20.0;
        let params = Params::new(&recipe);
        assert!((params.contrast - 0.35).abs() < 1e-6, "{}", params.contrast);
        assert!((params.grain - 0.28).abs() < 1e-6, "{}", params.grain);
        recipe.look.as_mut().unwrap().strength = 50.0;
        assert!((Params::new(&recipe).contrast - 0.225).abs() < 1e-6);
        // Values stay in their ranges.
        recipe.saturation = 100.0;
        assert!(Params::new(&recipe).saturation <= 1.0);

        // A curve that darkens everything darkens the look's result.
        let looked = develop(&with_look(Look::Portra400, 100.0, 0.0), [0.5; 3]);
        let mut darker = with_look(Look::Portra400, 100.0, 0.0);
        darker.curves.all.points = vec![[0.0, 0.0], [1.0, 0.5]];
        let out = develop(&darker, [0.5; 3]);
        for c in 0..3 {
            assert!(out[c] < looked[c] - 0.1, "{out:?} vs {looked:?}");
        }
    }

    #[test]
    fn req_028_04_the_look_is_saved_with_the_edit() {
        let mut recipe = with_look(Look::Hp5, 60.0, 150.0);
        recipe.exposure = 0.5;
        let json = recipe.to_json();
        assert!(
            json.contains(r#""look":{"look":"hp5","strength":60.0,"grain":150.0}"#),
            "{json}"
        );
        assert_eq!(Recipe::from_json(&json), Some(recipe));
        // Without a look nothing changes in the JSON; a look only a newer version knows, and values out of range.
        assert!(!Recipe::default().to_json().contains("look"));
        let unknown = r#"{"version":1,"look":{"look":"ektachrome","strength":50,"grain":100}}"#;
        assert_eq!(Recipe::from_json(unknown).unwrap().look, None);
        let hostile = r#"{"version":1,"look":{"look":"velvia50","strength":500,"grain":-3}}"#;
        let look = Recipe::from_json(hostile).unwrap().look.unwrap();
        assert_eq!((look.strength, look.grain), (100.0, 0.0));
    }
}
