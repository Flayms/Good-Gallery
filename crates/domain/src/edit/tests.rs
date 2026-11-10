//! The photo editing formulas' tests.

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
