use super::gpu::{
    EditResources, PaintTexture, PreviewCallback, added_to, cpu_preview, cpu_preview_lut, uniform, upload_lut,
};
use super::load::fit_small;
use super::*;
use domain::edit::{Adjustment, BlurKind, LocalMask, MaskShape};
use domain::paint::{Painting, Stroke};
use eframe::egui::{Pos2, Vec2, pos2, vec2};
use eframe::egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};

const SIZE: u32 = 48;

/// A wgpu device, one test at a time: devices on parallel test threads crash the test process (lavapipe, WARP).
fn render_state() -> (std::sync::MutexGuard<'static, ()>, egui_wgpu::RenderState) {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = GPU.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let render_state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions::default(),
    );
    (guard, render_state)
}

/// Colors across the range, with a ramp, saturated and skin-like patches.
fn pattern() -> image::RgbaImage {
    image::RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let t = x as f32 / (SIZE - 1) as f32;
        let rgb = match y * 4 / SIZE {
            0 => [t, t, t],
            1 => [t, 0.3, 1.0 - t],
            2 => [0.85, 0.65 * t + 0.2, 0.5],
            _ => [0.1 + 0.8 * t, 0.9 - 0.5 * t, 0.2],
        };
        let [r, g, b] = rgb.map(|c| (c * 255.0).round() as u8);
        image::Rgba([r, g, b, 255])
    })
}

fn recipes() -> Vec<Recipe> {
    let mut all = Recipe::default();
    for (adjustment, value) in [
        (Adjustment::Exposure, 0.7),
        (Adjustment::Brightness, 35.0),
        (Adjustment::Contrast, 40.0),
        (Adjustment::Highlights, -50.0),
        (Adjustment::Shadows, 60.0),
        (Adjustment::Whites, 20.0),
        (Adjustment::Blacks, -30.0),
        (Adjustment::Temperature, 30.0),
        (Adjustment::Tint, -20.0),
        (Adjustment::Hue, 40.0),
        (Adjustment::Vibrance, 50.0),
        (Adjustment::Saturation, -20.0),
        (Adjustment::Vignette, -60.0),
        (Adjustment::Grain, 40.0),
    ] {
        adjustment.set(&mut all, value);
    }
    all.curves.all.add_point(0.3, 0.2);
    all.curves.blue.add_point(0.6, 0.7);
    let mut light = Recipe::default();
    Adjustment::Vignette.set(&mut light, 70.0);
    Adjustment::Contrast.set(&mut light, -50.0);
    Adjustment::Brightness.set(&mut light, -60.0);
    // The hue alone, half way round (REQ-021-20).
    let mut hue = Recipe::default();
    Adjustment::Hue.set(&mut hue, -180.0);
    vec![Recipe::default(), all, light, hue]
}

/// Renders `image` through `recipe` with the shader, at 1:1, and reads it back.
fn render_gpu(
    render_state: &egui_wgpu::RenderState,
    image: &image::RgbaImage,
    recipe: &Recipe,
    mask: Option<&Mask>,
    flags: u32,
) -> Vec<u8> {
    render_gpu_lut(render_state, image, recipe, mask, flags, None)
}

/// [`render_gpu`] with the table of the recipe's LUT.
fn render_gpu_lut(
    render_state: &egui_wgpu::RenderState,
    image: &image::RgbaImage,
    recipe: &Recipe,
    mask: Option<&Mask>,
    flags: u32,
    lut: Option<Arc<domain::lut::Lut>>,
) -> Vec<u8> {
    let (device, queue) = (&render_state.device, &render_state.queue);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let resources = EditResources::new(device, format);
    let (width, height) = image.dimensions();
    let size = [width, height];
    let levels = crate::textures::mip_chain(size, image.as_raw().clone());
    let source = crate::textures::upload(device, queue, size, &levels);
    let view = source.create_view(&wgpu::TextureViewDescriptor::default());

    let (output_width, output_height) = recipe.geometry().output_size(width, height);
    let params = Params::new(recipe).with_lut(lut);
    let lut_view = params.lut.as_ref().map(|(table, _)| upload_lut(device, queue, table));
    let uniform = uniform(
        &params,
        recipe.geometry(),
        [0.0, 0.0, output_width as f32, output_height as f32],
        [0.0, 0.0, 1.0, 1.0],
        [width as f32, height as f32],
        1.0,
        flags as f32,
    );
    let mask = mask.map(|mask| upload_mask(render_state, mask));
    let mut paint = PaintTexture::new(device, [width as f32, height as f32]);
    paint.sync(queue, &recipe.paintings);
    let mut lens_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let lens = recipe
        .blur_lens
        .then(|| resources.build_lens(device, &mut lens_encoder, &view))
        .flatten();
    queue.submit([lens_encoder.finish()]);
    let group = resources.bind(
        device,
        &view,
        mask.as_ref(),
        Some(&paint.view),
        lens.as_ref(),
        lut_view.as_ref(),
        &uniform,
    );
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("edit test"),
        size: wgpu::Extent3d {
            width: output_width,
            height: output_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let (width, height) = (output_width, output_height);
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("edit test"),
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("edit test"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&resources.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("mapped"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("polled");
    let data = slice.get_mapped_range().expect("mapped");
    let pixels = (0..height as usize)
        .flat_map(|y| data[y * row as usize..y * row as usize + width as usize * 4].to_vec())
        .collect();
    drop(data);
    buffer.unmap();
    pixels
}

/// REQ-021-02, REQ-021-06, REQ-021-20, REQ-022-03, REQ-022-04: the GPU preview shows what saving renders on the CPU
/// (within two levels), grain, vignette, the hue, the subject's and the background's own adjustments and the
/// selection's tint included.
#[test]
fn req_021_02_req_021_06_the_gpu_preview_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let image = pattern();
    // The subject grows from left to right, in steps the GPU's 8-bit mask holds exactly.
    let values = (0..SIZE * SIZE).map(|i| ((i % SIZE) * 5) as f32 / 255.0).collect();
    let mask = Mask::new(SIZE, SIZE, values).unwrap();
    let mut parts = recipes()[1].clone();
    Adjustment::Exposure.set(parts.part_mut(Part::Subject), 1.2);
    Adjustment::Saturation.set(parts.part_mut(Part::Subject), 40.0);
    Adjustment::Shadows.set(parts.part_mut(Part::Background), -50.0);
    Adjustment::Temperature.set(parts.part_mut(Part::Background), -60.0);
    Adjustment::Hue.set(parts.part_mut(Part::Subject), 90.0);
    let mut cases: Vec<(Recipe, Option<&Mask>, u32)> = recipes().into_iter().map(|recipe| (recipe, None, 0)).collect();
    for flags in [4, 4 | 8, 4 | 8 | 16] {
        cases.push((parts.clone(), Some(&mask), flags));
    }
    for (recipe, mask, flags) in cases {
        let gpu = render_gpu(&render_state, &image, &recipe, mask, flags);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], mask, flags);
        let mut worst = 0;
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(
                    diff <= 2,
                    "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with {recipe:?}, flags {flags}"
                );
                worst = worst.max(diff);
            }
        }
        eprintln!("worst difference: {worst}");
    }
}

/// REQ-029-01, REQ-029-04: every turn and a crop preview as saving renders them, with the vignette on the crop.
#[test]
fn req_029_04_the_gpu_preview_of_crop_and_turns_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let image = pattern();
    for (crop, turns) in [
        (None, 1),
        (None, 2),
        (None, 3),
        (Some([0.0, 0.5, 0.5, 0.5]), 0),
        (Some([0.5, 0.0, 0.5, 0.5]), 1),
    ] {
        let mut recipe = recipes()[1].clone();
        Adjustment::Vignette.set(&mut recipe, -60.0);
        recipe.crop = crop;
        recipe.quarter_turns = turns;
        let gpu = render_gpu(&render_state, &image, &recipe, None, 0);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, 0);
        let (width, height) = recipe.geometry().output_size(SIZE, SIZE);
        assert_eq!(cpu.size, [width as usize, height as usize], "{crop:?} {turns}");
        assert_eq!(gpu.len(), cpu.pixels.len() * 4, "{crop:?} {turns}");
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(
                    diff <= 3,
                    "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?}, {crop:?} {turns}"
                );
            }
        }
    }
}

/// REQ-029-10, REQ-029-11 (#248): flips and straightening preview as saving renders them, with the vignette on
/// the output. Flips move whole pixels; straightened, the GPU samples between pixels where the CPU's small copy
/// takes the nearest, so most pixels agree closely and the bands' edges a little less. The aspect of 1.5 on a
/// square pattern checks that both follow the same formula for the canvas.
#[test]
fn req_029_10_req_029_11_the_gpu_preview_of_flips_and_straightening_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let image = pattern();
    for (degrees, flip, turns) in [
        (0.0, Some(true), 0),
        (0.0, Some(false), 1),
        (12.5, None, 0),
        (-30.0, Some(true), 3),
        (95.0, Some(false), 2),
    ] {
        let mut recipe = recipes()[1].clone();
        Adjustment::Vignette.set(&mut recipe, -60.0);
        recipe.quarter_turns = turns;
        recipe.set_straighten(degrees, 1.5);
        if let Some(horizontal) = flip {
            recipe.flip(horizontal);
        }
        let gpu = render_gpu(&render_state, &image, &recipe, None, 0);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, 0);
        let (width, height) = recipe.geometry().output_size(SIZE, SIZE);
        let case = format!("{degrees}° {flip:?} {turns}");
        assert_eq!(cpu.size, [width as usize, height as usize], "{case}");
        assert_eq!(gpu.len(), cpu.pixels.len() * 4, "{case}");
        let (limit, share) = if degrees == 0.0 { (3, 1.0) } else { (8, 0.85) };
        let close = gpu
            .chunks_exact(4)
            .zip(cpu.pixels.iter())
            .filter(|(gpu, cpu)| (0..3).all(|c| (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs() <= limit))
            .count();
        assert!(
            close as f32 >= share * cpu.pixels.len() as f32,
            "{case}: {close} of {} pixels agree",
            cpu.pixels.len()
        );
    }
}

/// REQ-028-05: a film look previews as saving renders it (within two levels), at any strength and grain, with
/// other adjustments and curves on top.
#[test]
fn req_028_05_the_gpu_preview_of_a_look_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let image = pattern();
    for (i, look) in domain::look::Look::ALL.into_iter().enumerate() {
        let mut recipe = recipes()[i % recipes().len()].clone();
        recipe.look = Some(domain::look::LookChoice {
            look,
            strength: [100.0, 70.0, 35.0][i % 3],
            grain: [100.0, 200.0, 0.0][i % 3],
        });
        let gpu = render_gpu(&render_state, &image, &recipe, None, 0);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, 0);
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(
                    diff <= 2,
                    "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with {recipe:?}"
                );
            }
        }
    }
}

/// REQ-043-02: a LUT previews as saving renders it (within two levels), at any strength, under other adjustments.
#[test]
fn req_043_02_the_gpu_preview_of_a_lut_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let image = pattern();
    // 17 points a side, bending each channel its own way.
    let n = 17;
    let mut text = format!("TITLE \"Bend\"\nLUT_3D_SIZE {n}\n");
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let (r, g, b) = [r, g, b].map(|v| v as f32 / (n - 1) as f32).into();
                text.push_str(&format!("{} {} {}\n", r.sqrt(), g * g, 0.2 + 0.6 * b));
            }
        }
    }
    let lut = Arc::new(domain::lut::Lut::parse(&text).unwrap());
    for (i, strength) in [100.0, 60.0, 25.0].into_iter().enumerate() {
        let mut recipe = recipes()[i % recipes().len()].clone();
        recipe.lut = Some(domain::lut::LutChoice {
            id: domain::lut::lut_id(text.as_bytes()),
            name: "Bend".into(),
            strength,
        });
        let gpu = render_gpu_lut(&render_state, &image, &recipe, None, 0, Some(lut.clone()));
        let cpu = cpu_preview_lut(
            &image,
            &recipe,
            recipe.geometry(),
            [SIZE, SIZE],
            None,
            0,
            Some(lut.clone()),
        );
        let plain = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, 0);
        assert_ne!(cpu.pixels, plain.pixels, "the LUT changes the photo");
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(
                    diff <= 2,
                    "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} at {strength} %"
                );
            }
        }
    }
}

/// REQ-023-02, REQ-023-03, REQ-023-04: the preview blurs as saving does (within three levels), deep into the
/// pyramid, around a focus and behind a subject.
#[test]
fn req_023_04_the_gpu_blur_matches_the_cpu_blur() {
    let (_gpu, render_state) = render_state();
    let (width, height) = (512, 384);
    // Hard edges: a checker of 16 px with a color ramp.
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        let on = (x / 16 + y / 16) % 2 == 0;
        let t = (x * 255 / width) as u8;
        if on {
            image::Rgba([t, 200, 40, 255])
        } else {
            image::Rgba([20, 60, 255 - t, 255])
        }
    });
    let values = (0..width * height)
        .map(|i| if i % width < width / 2 { 1.0 } else { 0.0 })
        .collect();
    // The subject is the left half, in a mask as large as the photo.
    let mask = Mask::new(width, height, values).unwrap();
    let blur = |kind: BlurKind, focus: [f32; 2], size: f32| Recipe {
        blur_kind: kind,
        blur_focus: focus,
        blur: 100.0,
        blur_size: size,
        ..Recipe::default()
    };
    let mut lit = blur(BlurKind::Radial, [0.3, 0.6], 10.0);
    Adjustment::Exposure.set(&mut lit, 0.5);
    // Lens blur (REQ-023-05): discs from the lens pyramid, at a strong and a weak strength.
    let lens = |mut recipe: Recipe, strength: f32| {
        recipe.blur_lens = true;
        recipe.blur = strength;
        recipe
    };
    let cases = [
        (blur(BlurKind::TiltShift, [0.5, 0.3], 20.0), None, 0),
        (lit, None, 0),
        (blur(BlurKind::Background, [0.5, 0.5], 30.0), Some(&mask), 4),
        (lens(blur(BlurKind::Radial, [0.3, 0.6], 10.0), 100.0), None, 0),
        (lens(blur(BlurKind::Radial, [0.3, 0.6], 10.0), 15.0), None, 0),
        (lens(blur(BlurKind::Background, [0.5, 0.5], 30.0), 60.0), Some(&mask), 4),
    ];
    for (recipe, mask, flags) in cases {
        let gpu = render_gpu(&render_state, &image, &recipe, mask, flags);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [width, height], mask, flags);
        let mut worst = 0;
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(
                    diff <= 3,
                    "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with {recipe:?}"
                );
                worst = worst.max(diff);
            }
        }
        eprintln!("worst difference: {worst}");
        // And it does blur: many pixels of the checker change a lot.
        let changed = gpu
            .chunks_exact(4)
            .zip(image.pixels())
            .filter(|(gpu, source)| (i32::from(gpu[1]) - i32::from(source[1])).abs() > 40)
            .count();
        assert!(
            changed > (width * height / 10) as usize,
            "{changed} changed with {recipe:?}"
        );
    }
}

/// REQ-028-05, REQ-028-08: the preview's halation matches the saved render, on a photo large enough for it to reach
/// beyond a pixel, and it does glow red beside a light.
#[test]
fn req_028_08_the_gpu_halation_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let (width, height) = (512, 384);
    let image = image::RgbaImage::from_fn(width, height, |x, _| {
        let v = if (250..262).contains(&x) { 255 } else { 25 };
        image::Rgba([v, v, v, 255])
    });
    let recipe = Recipe {
        look: Some(domain::look::LookChoice {
            grain: 0.0,
            ..domain::look::LookChoice::new(domain::look::Look::Gold200)
        }),
        ..Recipe::default()
    };
    let gpu = render_gpu(&render_state, &image, &recipe, None, 0);
    let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [width, height], None, 0);
    for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
        for c in 0..3 {
            let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
            assert!(diff <= 3, "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?}");
        }
    }
    let red = |x: usize| i32::from(gpu[(height as usize / 2 * width as usize + x) * 4]);
    assert!(
        red(248) > red(100) + 8,
        "beside the light {} vs far {}",
        red(248),
        red(100)
    );
}

/// REQ-030-07, REQ-030-08: every kind of mask, and its tint, previews as saving renders it (within two levels).
#[test]
fn req_030_07_req_030_08_the_gpu_preview_of_masks_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let image = pattern();
    let mask = |shape: MaskShape, invert: bool, adjustment: Adjustment, value: f32| {
        let mut mask = LocalMask::new(shape);
        mask.invert = invert;
        adjustment.set(&mut mask.recipe, value);
        mask
    };
    let linear = MaskShape::Linear {
        from: [0.2, 0.1],
        to: [0.7, 0.8],
    };
    let luminance = MaskShape::Luminance {
        low: 30.0,
        high: 70.0,
        feather: 10.0,
    };
    let color = MaskShape::Color {
        hue: 30.0,
        range: 60.0,
        feather: 30.0,
    };
    let recipe = Recipe {
        masks: vec![
            mask(linear, false, Adjustment::Exposure, 1.0),
            mask(luminance, false, Adjustment::Temperature, 60.0),
            mask(color, false, Adjustment::Saturation, -80.0),
            mask(linear, true, Adjustment::Contrast, 40.0),
        ],
        ..Recipe::default()
    };
    let plain = render_gpu(&render_state, &image, &Recipe::default(), None, 0);
    for flags in [0, 1 << 8, 2 << 8, 3 << 8, 4 << 8] {
        let gpu = render_gpu(&render_state, &image, &recipe, None, flags);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, flags);
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(
                    diff <= 2,
                    "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with flags {flags}"
                );
            }
        }
        assert_ne!(gpu, plain, "the masks change the photo");
    }
    // Cropped and turned, a gradient stays on its part of the photo.
    let mut turned = recipe;
    turned.quarter_turns = 1;
    turned.crop = Some([0.5, 0.0, 0.5, 0.5]);
    let gpu = render_gpu(&render_state, &image, &turned, None, 0);
    let cpu = cpu_preview(&image, &turned, turned.geometry(), [SIZE, SIZE], None, 0);
    for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
        for c in 0..3 {
            let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
            assert!(diff <= 3, "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?}");
        }
    }
}

/// REQ-030-12, REQ-030-13: brushed and outlined shapes preview as saving renders them, also combined.
#[test]
fn req_030_12_req_030_13_the_gpu_preview_of_paintings_matches_the_cpu_render() {
    let (_gpu, render_state) = render_state();
    let image = pattern();
    let brushed = Painting {
        strokes: vec![
            Stroke {
                points: vec![[0.1, 0.2], [0.5, 0.6], [0.9, 0.3]],
                size: 8.0,
                softness: 60.0,
                erase: false,
                closed: false,
            },
            Stroke {
                points: vec![[0.5, 0.0], [0.5, 1.0]],
                size: 4.0,
                softness: 0.0,
                erase: true,
                closed: false,
            },
        ],
    };
    let outlined = Painting {
        strokes: vec![Stroke {
            points: vec![[0.2, 0.2], [0.8, 0.3], [0.6, 0.9], [0.1, 0.7]],
            size: 0.0,
            softness: 40.0,
            erase: false,
            closed: true,
        }],
    };
    let mut first = LocalMask::new(MaskShape::Painted {
        layer: 0,
        freeform: false,
    });
    Adjustment::Exposure.set(&mut first.recipe, 1.0);
    let mut second = LocalMask::new(MaskShape::Painted {
        layer: 1,
        freeform: true,
    });
    Adjustment::Saturation.set(&mut second.recipe, -90.0);
    let recipe = Recipe {
        paintings: vec![brushed, outlined],
        masks: vec![first, second],
        ..Recipe::default()
    };
    let plain = render_gpu(&render_state, &image, &Recipe::default(), None, 0);
    for flags in [0, 1 << 8, 2 << 8] {
        let gpu = render_gpu(&render_state, &image, &recipe, None, flags);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, flags);
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(
                    diff <= 2,
                    "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with flags {flags}"
                );
            }
        }
        assert_ne!(gpu, plain, "the paintings change the photo");
    }
}

/// REQ-030-12: the layer the preview keeps up to date while the user paints holds what drawing it anew gives.
#[test]
fn req_030_12_the_texture_kept_up_to_date_equals_one_drawn_anew() {
    let (_gpu, render_state) = render_state();
    let (device, queue) = (&render_state.device, &render_state.queue);
    let points = [[0.1, 0.2], [0.3, 0.7], [0.6, 0.4], [0.9, 0.8]];
    let stroke = |count: usize, erase: bool| Stroke {
        points: points[..count].to_vec(),
        // The eraser is narrower, so that some of the stroke stays.
        size: if erase { 1.0 } else { 4.0 },
        softness: 40.0,
        erase,
        closed: false,
    };
    let full = Painting {
        strokes: vec![stroke(4, false), stroke(4, true)],
    };
    let mut live = PaintTexture::new(device, [400.0, 300.0]);
    for count in 1..=4 {
        let painting = Painting {
            strokes: vec![stroke(count, false)],
        };
        live.sync(queue, std::slice::from_ref(&painting));
    }
    for count in 1..=4 {
        let painting = Painting {
            strokes: vec![stroke(4, false), stroke(count, true)],
        };
        live.sync(queue, std::slice::from_ref(&painting));
    }
    let mut anew = PaintTexture::new(device, [400.0, 300.0]);
    anew.sync(queue, std::slice::from_ref(&full));
    assert_eq!(live.drawn[0].1, anew.drawn[0].1);
    assert!(live.drawn[0].1.iter().any(|&value| value > 0.5));

    // Taken away again, the layer is empty.
    live.sync(queue, &[]);
    assert!(live.drawn[0].1.iter().all(|&value| value == 0.0));
}

/// REQ-030-12: the texture kept up to date stroke by stroke shows what drawing it all at once shows.
#[test]
fn req_030_12_the_preview_adds_only_whats_new() {
    let stroke = |count: usize| Stroke {
        points: [[0.1, 0.1], [0.5, 0.2], [0.8, 0.8]][..count].to_vec(),
        size: 5.0,
        softness: 50.0,
        erase: false,
        closed: false,
    };
    let painting = |strokes: Vec<Stroke>| Painting { strokes };
    let one = stroke(1);
    let two = stroke(2);
    assert_eq!(
        added_to(&painting(vec![]), &painting(vec![one.clone()])),
        Some((&one, 0)),
        "a new stroke"
    );
    let longer = painting(vec![two.clone()]);
    assert_eq!(
        added_to(&painting(vec![one.clone()]), &longer),
        Some((&two, 1)),
        "more of it"
    );
    // Anything else is drawn anew: fewer strokes, another one before it, thinned, or an outline.
    assert_eq!(added_to(&longer, &painting(vec![])), None);
    let moved = Stroke {
        points: vec![[0.9, 0.9], [0.5, 0.2]],
        ..two.clone()
    };
    assert_eq!(added_to(&painting(vec![one.clone()]), &painting(vec![moved])), None);
    let outline = Stroke { closed: true, ..two };
    let shorter = Stroke { closed: true, ..one };
    assert_eq!(added_to(&painting(vec![shorter]), &painting(vec![outline])), None);
}

#[test]
fn req_021_07_req_021_08_tracks_changes_undo_and_asks_before_leaving() {
    let target = Target {
        media: 1,
        root_id: 1,
        rel_path: "a.jpg".into(),
        file_name: "a.jpg".into(),
        library: PathBuf::from("/photos"),
        full_size: [10, 10],
        sidecar_orientation: None,
    };
    let mut session = EditSession::new(target, None);
    session.base = Base::Loaded(Box::new(Loaded {
        view: None,

        lens: None,
        small: Some(image::RgbaImage::from_pixel(10, 10, image::Rgba([200, 150, 100, 255]))),
        cpu: None,
    }));
    session.source = Some("a.jpg".into());
    assert_eq!(session.leave(Leave::Close), Some(Outcome::Leave(Leave::Close)));

    let mut recipe = Recipe::default();
    Adjustment::Exposure.set(&mut recipe, 1.0);
    session.commit(recipe);
    assert!(session.has_changes());
    assert_eq!(session.leave(Leave::Step(3)), None);
    assert_eq!(session.dialog, Some(Dialog::Leave(Leave::Step(3))));
    session.undo();
    assert!(!session.has_changes());
    session.redo();
    assert_eq!(session.recipe().exposure, 1.0);

    // The first save over the original asks; confirmed, it saves.
    session.dialog = None;
    assert_eq!(session.save(SaveMode::Replace, None), None);
    assert_eq!(session.dialog, Some(Dialog::Replace(None)));
    let Some(Outcome::Save(request, None)) = session.save(SaveMode::Replace, None) else {
        panic!("saves once confirmed")
    };
    assert_eq!((request.source.as_str(), request.copy), ("a.jpg", false));
    assert!(session.saving);
    // While saving, nothing else starts.
    assert_eq!(session.save(SaveMode::Copy, None), None);
    assert_eq!(session.leave(Leave::Exit), None);
}

/// REQ-021-08 (#69): _Discard_ ends editing after the frame's preview was queued; drawing it must still work.
#[test]
fn req_021_08_discarding_after_the_preview_was_queued_still_draws_it() {
    let (_gpu, render_state) = render_state();
    let (device, queue) = (&render_state.device, &render_state.queue);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let size = [SIZE, SIZE];
    let texture = crate::textures::upload(
        device,
        queue,
        size,
        &crate::textures::mip_chain(size, pattern().into_raw()),
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let target = Target {
        media: 1,
        root_id: 1,
        rel_path: "a.jpg".into(),
        file_name: "a.jpg".into(),
        library: PathBuf::from("/photos"),
        full_size: size,
        sidecar_orientation: None,
    };
    let mut session = EditSession::new(target, Some(render_state.clone()));
    session.base = Base::Loaded(Box::new(Loaded {
        view: Some(view.clone()),

        lens: None,
        small: Some(pattern()),
        cpu: None,
    }));
    let callback = PreviewCallback {
        view,
        mask: None,
        paintings: None,
        lens: None,
        lut: None,
        uniform: uniform(
            &Params::new(&Recipe::default()),
            Geometry::new(None, 0),
            [0.0, 0.0, SIZE as f32, SIZE as f32],
            [0.0, 0.0, 1.0, 1.0],
            [SIZE as f32; 2],
            1.0,
            0.0,
        ),
        group: std::sync::OnceLock::new(),
    };
    // Discard: the session ends, then the frame is drawn.
    drop(session);
    let mut resources = CallbackResources::default();
    resources.insert(EditResources::new(device, format));
    let screen = ScreenDescriptor {
        size_in_pixels: size,
        pixels_per_point: 1.0,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    callback.prepare(device, queue, &screen, &mut encoder, &mut resources);
    let frame = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("edit test"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let frame_view = frame.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("edit test"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let mut pass = pass.forget_lifetime();
        let info = egui::PaintCallbackInfo {
            viewport: Rect::from_min_size(Pos2::ZERO, Vec2::splat(SIZE as f32)),
            clip_rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(SIZE as f32)),
            pixels_per_point: 1.0,
            screen_size_px: size,
        };
        callback.paint(info, &mut pass, &resources);
    }
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::wait_indefinitely()).expect("polled");
}

#[test]
fn req_021_05_the_picker_neutralizes_the_picked_spot() {
    let target = Target {
        media: 1,
        root_id: 1,
        rel_path: "a.jpg".into(),
        file_name: "a.jpg".into(),
        library: PathBuf::from("/photos"),
        full_size: [10, 10],
        sidecar_orientation: None,
    };
    let mut session = EditSession::new(target, None);
    let warm = image::RgbaImage::from_pixel(10, 10, image::Rgba([190, 160, 130, 255]));
    session.base = Base::Loaded(Box::new(Loaded {
        view: None,

        lens: None,
        small: Some(warm.clone()),
        cpu: None,
    }));
    session.picking = true;
    session.pick(pos2(0.5, 0.5));
    assert!(!session.is_picking());
    let out = Params::new(session.recipe()).pixel([190.0 / 255.0, 160.0 / 255.0, 130.0 / 255.0], [0.5; 2], [10.0; 2]);
    assert!(
        (out[0] - out[2]).abs() < 0.02 && (out[0] - out[1]).abs() < 0.02,
        "{out:?}"
    );
}

/// A target whose file isn't there, so a test sees when it would be read.
fn missing_target(full_size: [u32; 2]) -> Target {
    Target {
        media: 1,
        root_id: 1,
        rel_path: "missing.jpg".into(),
        file_name: "missing.jpg".into(),
        library: std::env::temp_dir().join("good-gallery-no-library"),
        full_size,
        sidecar_orientation: None,
    }
}

/// REQ-022-05, REQ-021-07 (#151): editing a photo saved with a background part again starts with nothing to undo,
/// and shows the saved file (no preview, the file's size) until the subject is found again instead of the edit
/// without its parts; _Compare_ shows the original meanwhile.
#[test]
fn req_022_05_req_021_07_shows_the_saved_file_until_the_subject_is_found_again() {
    let mut recipe = Recipe::default();
    Adjustment::Hue.set(recipe.part_mut(Part::Background), -79.0);
    let mut session = EditSession::new(missing_target([10, 10]), None);
    session.set_source("a.jpg".into(), recipe.clone());
    assert!(!session.history.can_undo() && !session.history.can_redo());
    session.undo();
    assert_eq!(session.recipe(), &recipe);
    session.base = Base::Loaded(Box::new(Loaded {
        view: None,

        lens: None,
        small: Some(image::RgbaImage::from_pixel(10, 10, image::Rgba([60, 140, 60, 255]))),
        cpu: None,
    }));
    let (tx, rx) = channel();
    session.subject = Subject::Finding(rx);

    let ctx = egui::Context::default();
    let paint = |session: &mut EditSession, compare: bool| {
        let mut drawn = false;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0));
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            drawn = session.paint(ui, rect, uv, rect, compare, 1.0);
        })
        .textures_delta
        .clear();
        drawn
    };
    assert!(!paint(&mut session, false), "the saved file shows while finding");
    assert_eq!(session.shown_size(false), None);
    assert!(
        !paint(&mut session, true),
        "compare shows the saved file too: it is how editing started"
    );
    assert_eq!(session.shown_size(true), None);

    tx.send(Mask::new(10, 10, vec![1.0; 100]).ok_or(String::from("mask")))
        .unwrap();
    session.poll();
    assert!(paint(&mut session, false), "the preview with its parts once found");
    assert_eq!(session.shown_size(false), Some([10.0, 10.0]));
}

/// REQ-021-07, REQ-029-05 (#247): _Compare_ shows the photo as this editing session started: a saved photo with its
/// saved edit, crop included, and one without an edit as the original.
#[test]
fn req_021_07_req_029_05_compare_shows_the_photo_as_editing_started() {
    let ctx = egui::Context::default();
    let paint = |session: &mut EditSession, compare: bool| -> Option<(Recipe, Geometry, u32)> {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0));
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            assert!(session.paint(ui, rect, uv, rect, compare, 1.0));
        })
        .textures_delta
        .clear();
        let Base::Loaded(loaded) = &session.base else {
            return None;
        };
        loaded
            .cpu
            .as_ref()
            .map(|(recipe, geometry, flags, _)| (recipe.clone(), *geometry, *flags))
    };
    let loaded = || {
        Base::Loaded(Box::new(Loaded {
            view: None,
            lens: None,
            small: Some(image::RgbaImage::from_pixel(10, 10, image::Rgba([90, 90, 90, 255]))),
            cpu: None,
        }))
    };

    // Saved with the left half kept: the 10 × 10 file comes from a 20 × 10 original.
    let saved = Recipe {
        exposure: 1.0,
        crop: Some([0.0, 0.0, 0.5, 1.0]),
        ..Recipe::default()
    };
    let mut session = EditSession::new(missing_target([10, 10]), None);
    session.set_source(".good-gallery/originals/a.jpg".into(), saved.clone());
    session.base = loaded();
    let changed = Recipe {
        exposure: 2.0,
        crop: None,
        ..saved.clone()
    };
    session.commit(changed.clone());

    let (recipe, geometry, flags) = paint(&mut session, true).unwrap();
    assert_eq!(recipe, saved, "the saved edit, not the original");
    assert_eq!(geometry, saved.geometry());
    assert_eq!(flags & 1, 0, "rendered through the saved edit");
    assert_eq!(session.shown_size(true), Some([10.0, 10.0]), "the saved crop's size");
    assert_eq!(paint(&mut session, false).unwrap().0, changed);
    assert_eq!(session.shown_size(false), Some([20.0, 10.0]));

    let mut session = EditSession::new(missing_target([10, 10]), None);
    session.set_source("a.jpg".into(), Recipe::default());
    session.base = loaded();
    session.commit(changed);
    let (recipe, geometry, flags) = paint(&mut session, true).unwrap();
    assert!(recipe.is_neutral());
    assert_eq!(geometry, Geometry::new(None, 0));
    assert_eq!(flags & 1, 1, "the original as it is");
}

/// REQ-021-19: without an earlier edit, the panel shows its sliders as soon as editing starts, before any pixels
/// arrived.
#[test]
fn req_021_19_the_panel_shows_its_sliders_at_once() {
    use egui_kittest::kittest::Queryable as _;
    let mut session = EditSession::new(missing_target([120, 80]), None);
    session.start(None, &egui::Context::default());
    assert!(session.is_ready());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(320.0, 900.0))
        .with_context_setup(crate::setup)
        .build_ui_state(
            |ui, session: &mut EditSession| {
                session.panel(ui, ui.max_rect());
            },
            session,
        );
    harness.run();
    assert!(!harness.state().is_loaded(), "no pixels yet");
    harness.get_by_label("Exposure");
    harness.get_by_label("Saturation");
}

/// REQ-021-19: editing takes the texture the viewer shows and reads its small copy back from it, without reading
/// the file (which isn't there here).
#[test]
fn req_021_19_uses_the_shown_original_instead_of_reading_the_file() {
    let (_gpu, render_state) = render_state();
    let size = [1200, 800];
    let image = image::RgbaImage::from_fn(size[0], size[1], |x, y| {
        image::Rgba([(x * 255 / size[0]) as u8, (y * 255 / size[1]) as u8, 120, 255])
    });
    let texture = crate::textures::upload(
        &render_state.device,
        &render_state.queue,
        size,
        &crate::textures::mip_chain(size, image.clone().into_raw()),
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let shown = OriginalState::Loaded(crate::originals::Loaded {
        id: egui::TextureId::default(),
        size,
        view: Some(view.clone()),
    });

    let mut session = EditSession::new(missing_target(size), Some(render_state.clone()));
    session.start(None, &egui::Context::default());
    // Still loading in the viewer: editing waits for it instead of reading the file itself.
    session.use_shown(&OriginalState::Loading, || panic!("nothing to lend yet"));
    session.wait();
    assert!(session.failed().is_none() && !session.is_loaded());

    session.use_shown(&shown, || Some(view.clone()));
    session.wait();
    assert_eq!(session.failed(), None);
    assert!(session.is_loaded());
    let small = session.small().unwrap();
    let expected = fit_small(&image);
    assert_eq!(small.dimensions(), expected.dimensions());
    for (got, want) in small.pixels().zip(expected.pixels()) {
        for c in 0..4 {
            assert!(got[c].abs_diff(want[c]) <= 3, "{got:?} vs {want:?}");
        }
    }
}
