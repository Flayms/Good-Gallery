//! The preview on the GPU (`edit.wgsl`, `lens.wgsl`) and the CPU render tests compare it with.

use super::{Base, EditSession, Subject};
use domain::edit::{BlurKind, CURVE_SAMPLES, Geometry, MAX_MASKS, Mask, Params, Part, Pyramid, Recipe, blur_lod};
use domain::paint::{MAX_PAINTINGS, Painting, Stroke};
use eframe::egui::{self, Color32, Rect};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use std::sync::Arc;

impl EditSession {
    /// Draws the photo through the recipe into `rect` (texture coordinates `uv`), clipped to `clip`; as editing started
    /// while `compare` (REQ-021-02, REQ-021-07). False while there is nothing to draw yet, or while the parts wait for the
    /// subject (REQ-022-05).
    pub fn paint(&mut self, ui: &egui::Ui, rect: Rect, uv: Rect, clip: Rect, compare: bool, opacity: f32) -> bool {
        if self.waits_for_subject(compare) {
            return false;
        }
        let geometry = self.shown_geometry(compare);
        let recipe = self.shown_recipe(compare).clone();
        // With the LUT's table, taken before the loaded photo is borrowed (REQ-043-02).
        let lut = self.lut_table(&recipe);
        let params = Params::new(&recipe).with_lut(lut.clone());
        // Without an earlier edit the original as it is, not through a neutral recipe (REQ-021-13).
        let original = compare && recipe == Recipe::default();
        let tints = self.tints_selection() && !compare;
        self.sync_paintings(&recipe.paintings);
        let paintings_view = self.paint_texture.as_ref().map(|texture| texture.view.clone());
        let Base::Loaded(loaded) = &mut self.base else {
            return false;
        };
        let ctx = ui.ctx();
        let (mask, mask_view) = match &self.subject {
            Subject::Found(mask, view) => (Some(mask.as_ref()), view.clone()),
            _ => (None, None),
        };
        // Parts by the mask (4); the selection tinted (8), the background's (16) (REQ-022-03, REQ-022-04).
        let mut flags = u32::from(original);
        // The chosen mask tinted, as index + 1 above the other flags (REQ-030-07).
        if let (Part::Mask(i), true) = (self.part, tints) {
            flags |= (i as u32 + 1) << 8;
        }
        if mask.is_some() {
            flags |= 4;
            if tints && matches!(self.part, Part::Subject | Part::Background) {
                flags |= 8 | if self.part == Part::Background { 16 } else { 0 };
            }
        }
        let lens_shown = recipe.blur_lens && recipe.blur_kind != BlurKind::None && !original;
        if let (Some(view), Some(render_state)) = (&loaded.view, &self.render_state)
            && render_state
                .renderer
                .read()
                .callback_resources
                .contains::<EditResources>()
        {
            // The lens pyramid the first time a lens blur shows (REQ-023-05).
            if lens_shown && loaded.lens.is_none() {
                let renderer = render_state.renderer.read();
                if let Some(res) = renderer.callback_resources.get::<EditResources>() {
                    let device = &render_state.device;
                    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                    loaded.lens = res.build_lens(device, &mut encoder, view);
                    render_state.queue.submit([encoder.finish()]);
                }
            }
            // The LUT's table on the GPU, the first time it shows (REQ-043-02).
            let lut_view = match &params.lut {
                Some((table, _)) => {
                    if self
                        .lut_texture
                        .as_ref()
                        .is_none_or(|(shown, _)| !Arc::ptr_eq(shown, table))
                    {
                        let view = upload_lut(&render_state.device, &render_state.queue, table);
                        self.lut_texture = Some((table.clone(), view));
                    }
                    self.lut_texture.as_ref().map(|(_, view)| view.clone())
                }
                None => None,
            };
            let ppp = ctx.pixels_per_point();
            let shown = rect.intersect(clip);
            if shown.is_positive() {
                let flags = flags | if render_state.target_format.is_srgb() { 2 } else { 0 };
                let uniform = uniform(
                    &params,
                    geometry,
                    [rect.min.x * ppp, rect.min.y * ppp, rect.max.x * ppp, rect.max.y * ppp],
                    [uv.min.x, uv.min.y, uv.max.x, uv.max.y],
                    self.target.full_size.map(|side| side as f32),
                    opacity,
                    flags as f32,
                );
                ui.painter()
                    .with_clip_rect(clip)
                    .add(egui_wgpu::Callback::new_paint_callback(
                        shown,
                        PreviewCallback {
                            view: view.clone(),
                            mask: mask_view,
                            paintings: paintings_view,
                            lens: loaded.lens.clone().filter(|_| lens_shown),
                            lut: lut_view,
                            uniform,
                            group: std::sync::OnceLock::new(),
                        },
                    ));
            }
            return true;
        }
        // Without a wgpu renderer: the small copy, rendered on the CPU when the recipe changed.
        let Some(small) = &loaded.small else { return false };
        if loaded
            .cpu
            .as_ref()
            .is_none_or(|(shown, shown_geometry, shown_flags, _)| {
                *shown != recipe || *shown_geometry != geometry || *shown_flags != flags
            })
        {
            let image = cpu_preview_lut(small, &recipe, geometry, self.target.full_size, mask, flags, lut);
            match &mut loaded.cpu {
                Some((shown, shown_geometry, shown_flags, handle)) => {
                    handle.set(image, egui::TextureOptions::LINEAR);
                    *shown = recipe;
                    *shown_geometry = geometry;
                    *shown_flags = flags;
                }
                None => {
                    let handle = ctx.load_texture("edit preview", image, egui::TextureOptions::LINEAR);
                    loaded.cpu = Some((recipe, geometry, flags, handle));
                }
            }
        }
        if let Some((_, _, _, handle)) = &loaded.cpu {
            ui.painter()
                .with_clip_rect(clip)
                .image(handle.id(), rect, uv, Color32::WHITE.gamma_multiply(opacity));
        }
        true
    }
}

/// `small` through `recipe`, on the CPU, with the shader's `flags` (but 2: the output is encoded).
#[cfg(test)]
pub(super) fn cpu_preview(
    small: &image::RgbaImage,
    recipe: &Recipe,
    geometry: Geometry,
    full_size: [u32; 2],
    mask: Option<&Mask>,
    flags: u32,
) -> egui::ColorImage {
    cpu_preview_lut(small, recipe, geometry, full_size, mask, flags, None)
}

/// [`cpu_preview`] with the table of the recipe's LUT (REQ-043-02).
pub(super) fn cpu_preview_lut(
    small: &image::RgbaImage,
    recipe: &Recipe,
    geometry: Geometry,
    full_size: [u32; 2],
    mask: Option<&Mask>,
    flags: u32,
    lut: Option<Arc<domain::lut::Lut>>,
) -> egui::ColorImage {
    let size = full_size.map(|side| side.max(1) as f32);
    let paintings = domain::paint::draw_all(&recipe.paintings, size);
    let params = Params::new(recipe).with_paintings(&paintings).with_lut(lut);
    let (width, height) = small.dimensions();
    // A background blur waits for the subject (REQ-023-02).
    let blurs = params.blur > 0.0 && flags & 1 == 0 && (params.blur_kind != BlurKind::Background || flags & 4 != 0);
    let glows = params.halation > 0.0 && flags & 1 == 0;
    let pyramid = (blurs || glows).then(|| {
        let pixels = small.pixels().map(|p| p.0.map(|c| f32::from(c) / 255.0)).collect();
        Pyramid::new(width, height, pixels)
    });
    // Lights spread into discs (REQ-023-05).
    let lens = (blurs && params.blur_lens).then(|| {
        let pixels = small
            .pixels()
            .map(|p| domain::edit::lens_weighted(p.0.map(|c| f32::from(c) / 255.0)))
            .collect();
        Pyramid::new(width, height, pixels)
    });
    let mut rgba = Vec::with_capacity(small.as_raw().len());
    for (x, y, pixel) in small.enumerate_pixels() {
        let uv = [(x as f32 + 0.5) / width as f32, (y as f32 + 0.5) / height as f32];
        let subject = mask.map_or(0.0, |mask| mask.sample(uv));
        let mut rgba_in = pixel.0.map(|c| f32::from(c) / 255.0);
        let mut glow = [0.0; 3];
        if let Some(pyramid) = &pyramid {
            // σ in pixels of the small copy.
            let scale = width as f32 / size[0];
            if glows {
                let lod = blur_lod(Params::halation_sigma(size) * scale, pyramid.levels());
                let around = pyramid.blurred(rgba_in, uv, lod);
                glow = params.halation_glow([rgba_in[0], rgba_in[1], rgba_in[2]], [around[0], around[1], around[2]]);
            }
            if blurs {
                let sigma = params.blur_sigma(uv, size, subject) * scale;
                let small_size = [width as f32, height as f32];
                rgba_in = domain::edit::blur_pixel(pyramid, lens.as_ref(), rgba_in, uv, sigma, small_size);
            }
        }
        let rgb = [rgba_in[0], rgba_in[1], rgba_in[2]];
        let mut out = match mask {
            _ if flags & 1 != 0 => rgb,
            Some(_) if flags & 4 != 0 => params.pixel_glowing(rgb, glow, uv, size, Some(subject)),
            _ => params.pixel_glowing(rgb, glow, uv, size, None),
        };
        if flags & 8 != 0 {
            let selected = if flags & 16 != 0 { 1.0 - subject } else { subject };
            out = tint(out, selected);
        }
        if let Some(mask) = (flags >> 8).checked_sub(1).and_then(|i| params.masks.get(i as usize)) {
            out = tint(out, mask.weight(rgb, uv, size));
        }
        rgba.extend(out.map(|c| (c * 255.0).round() as u8));
        rgba.push(pixel[3]);
    }
    let (out_width, out_height) = geometry.output_size(width, height);
    let mut output = Vec::with_capacity(out_width as usize * out_height as usize * 4);
    for y in 0..out_height {
        for x in 0..out_width {
            let [u, v] = geometry.to_original([
                (x as f32 + 0.5) / out_width as f32,
                (y as f32 + 0.5) / out_height as f32,
            ]);
            // The empty corners around a straightened photo (REQ-029-10).
            if !geometry.shows_photo_at([u, v]) {
                output.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let from_x = ((u * width as f32) as u32).min(width - 1) as usize;
            let from_y = ((v * height as f32) as u32).min(height - 1) as usize;
            let at = (from_y * width as usize + from_x) * 4;
            output.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([out_width as usize, out_height as usize], &output)
}

/// The selection's tint (REQ-022-04): towards red by how much a pixel is `selected`, as `edit.wgsl` does.
fn tint(rgb: [f32; 3], selected: f32) -> [f32; 3] {
    let red = [1.0, 0.25, 0.25];
    std::array::from_fn(|c| rgb[c] + (red[c] - rgb[c]) * 0.45 * selected)
}

/// One part's light and color in the shader's order: gains times exposure and `on`, tone, color.
fn part_floats(params: &Params, on: f32) -> [f32; 16] {
    let [r, g, b] = params.gains.map(|gain| gain * params.exposure);
    let (sin, cos) = params.hue.sin_cos();
    [
        r,
        g,
        b,
        on,
        params.black,
        params.white,
        params.shadows,
        params.highlights,
        params.contrast,
        params.saturation,
        params.vibrance,
        params.brightness,
        cos,
        sin,
        0.0,
        0.0,
    ]
}

/// The shader's uniform: `edit.wgsl`'s `Edit`, in its order.
pub(super) fn uniform(
    params: &Params,
    geometry: Geometry,
    rect: [f32; 4],
    uv: [f32; 4],
    full_size: [f32; 2],
    opacity: f32,
    flags: f32,
) -> Vec<f32> {
    let mut floats = Vec::with_capacity(116 + 28 * MAX_MASKS + CURVE_SAMPLES * 4);
    floats.extend_from_slice(&rect);
    floats.extend_from_slice(&uv);
    floats.extend_from_slice(&[full_size[0], full_size[1], opacity, flags]);
    floats.extend_from_slice(&part_floats(params, f32::from(u8::from(params.curves.is_some()))));
    floats.extend_from_slice(&[params.vignette, params.vignette_start, params.grain, params.grain_cell]);
    for part in [&params.subject, &params.background] {
        match part {
            Some(part) => floats.extend_from_slice(&part_floats(part, 1.0)),
            None => floats.extend_from_slice(&[0.0; 16]),
        }
    }
    let kind = BlurKind::ALL
        .iter()
        .position(|kind| *kind == params.blur_kind)
        .unwrap_or(0) as f32;
    floats.extend_from_slice(&[params.blur, kind, params.blur_size, params.blur_feather]);
    floats.extend_from_slice(&[
        params.blur_focus[0],
        params.blur_focus[1],
        f32::from(geometry.quarter_turns()),
        f32::from(u8::from(params.blur_lens)),
    ]);
    floats.extend_from_slice(&geometry.crop());
    // Straightening and flips (REQ-029-10, REQ-029-11).
    let radians = geometry.straighten().to_radians();
    let [horizontal, vertical] = geometry.flips();
    floats.extend_from_slice(&[
        radians.cos(),
        radians.sin(),
        geometry.aspect(),
        f32::from(u8::from(horizontal) | (u8::from(vertical) << 1)),
    ]);
    floats.extend_from_slice(&[
        f32::from(u8::from(params.grain_midtones)),
        params.grain_color,
        f32::from(u8::from(params.hue_nodes.is_some())),
        params.halation,
    ]);
    for i in 0..domain::look::HUE_NODES {
        let [turn, chroma, light] = params.hue_nodes.map_or([0.0, 1.0, 0.0], |nodes| nodes[i]);
        floats.extend_from_slice(&[turn, chroma, light, 0.0]);
    }
    floats.extend_from_slice(&[params.masks.len() as f32, 0.0, 0.0, 0.0]);
    for i in 0..MAX_MASKS {
        match params.masks.get(i) {
            Some(mask) => {
                floats.extend_from_slice(&part_floats(&mask.params, 1.0));
                floats.extend_from_slice(&[f32::from(u8::from(mask.invert)), 0.0, 0.0, 0.0]);
                let [kind, a, b, c, d] = mask.shape.numbers();
                floats.extend_from_slice(&[kind, 0.0, a, b, c, d, 0.0, 0.0]);
            }
            None => floats.extend(std::iter::repeat_n(0.0, 28)),
        }
    }
    for i in 0..CURVE_SAMPLES {
        let [r, g, b] = params.curves.as_ref().map_or([0.0; 3], |table| table[i]);
        floats.extend_from_slice(&[r, g, b, 0.0]);
    }
    // The LUT (REQ-043-02): its strength and size, and its domain's corners.
    match &params.lut {
        Some((table, strength)) => {
            let (min, max) = table.domain();
            floats.extend_from_slice(&[*strength, table.size as f32, 0.0, 0.0]);
            floats.extend_from_slice(&[min[0], min[1], min[2], 0.0, max[0], max[1], max[2], 0.0]);
        }
        None => floats.extend_from_slice(&[0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0]),
    }
    floats
}

/// A LUT's table as a 3D texture, which the GPU interpolates between its points (REQ-043-02).
pub(super) fn upload_lut(device: &wgpu::Device, queue: &wgpu::Queue, lut: &domain::lut::Lut) -> wgpu::TextureView {
    let side = lut.size as u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lut"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: side,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // Red runs along x, green along y, blue through the layers, as the table lists them.
    let texels: Vec<u8> = lut
        .table
        .iter()
        .flat_map(|[r, g, b]| [*r, *g, *b, 1.0])
        .flat_map(|value| half::f16::from_f32(value).to_le_bytes())
        .collect();
    queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 8),
            rows_per_image: Some(side),
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: side,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

pub(super) struct EditResources {
    pub(super) pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The mask while there is no subject: nothing is.
    no_subject: wgpu::TextureView,
    /// The paintings while there are none.
    no_paintings: wgpu::TextureView,
    /// The lens blur's pyramid while there is none.
    no_lens: wgpu::TextureView,
    /// The LUT while there is none.
    no_lut: wgpu::TextureView,
    /// What builds a lens pyramid from a photo (`lens.wgsl`): weighing, then halving (REQ-023-05).
    lens_layout: wgpu::BindGroupLayout,
    weigh: wgpu::RenderPipeline,
    halve: wgpu::RenderPipeline,
}

/// The lens pyramid's pixels: weighted values, beyond 1.
const LENS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

impl EditResources {
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("edit.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("edit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("edit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let no_subject = mask_texture(device, 1, 1).create_view(&wgpu::TextureViewDescriptor::default());
        let (lens_layout, weigh, halve) = lens_pipelines(device);
        let no_lens = lens_texture(device, [1, 1], 1).create_view(&wgpu::TextureViewDescriptor::default());
        let no_lut = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("no lut"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            no_lens,
            no_lut,
            lens_layout,
            weigh,
            halve,
            pipeline,
            layout,
            sampler,
            no_subject,
            no_paintings: PaintTexture::new(device, [1.0, 1.0]).view,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn bind(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        mask: Option<&wgpu::TextureView>,
        paintings: Option<&wgpu::TextureView>,
        lens: Option<&wgpu::TextureView>,
        lut: Option<&wgpu::TextureView>,
        uniform: &[f32],
    ) -> wgpu::BindGroup {
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("edit"),
                contents: &uniform.iter().flat_map(|f| f.to_ne_bytes()).collect::<Vec<u8>>(),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("edit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(mask.unwrap_or(&self.no_subject)),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(paintings.unwrap_or(&self.no_paintings)),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(lens.unwrap_or(&self.no_lens)),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(lut.unwrap_or(&self.no_lut)),
                },
            ],
        })
    }

    /// The lens pyramid of `photo`, built on `encoder`; `None` for a photo too small to have one (REQ-023-05).
    pub(super) fn build_lens(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        photo: &wgpu::TextureView,
    ) -> Option<wgpu::TextureView> {
        let texture = photo.texture();
        let levels = texture.mip_level_count();
        if levels < 2 {
            return None;
        }
        let size = [texture.width() / 2, texture.height() / 2];
        let lens = lens_texture(device, size, levels - 1);
        let level = |texture: &wgpu::Texture, level: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        for target in 0..levels - 1 {
            let (source, pipeline) = if target == 0 {
                (level(texture, 0), &self.weigh)
            } else {
                (level(&lens, target - 1), &self.halve)
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("edit lens"),
                layout: &self.lens_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source),
                }],
            });
            let target = level(&lens, target);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("edit lens"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
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
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        Some(lens.create_view(&wgpu::TextureViewDescriptor::default()))
    }
}

/// A texture for a lens pyramid of `levels` levels.
fn lens_texture(device: &wgpu::Device, [width, height]: [u32; 2], levels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("edit lens"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LENS_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

/// The bind group layout and the two pipelines of `lens.wgsl`.
fn lens_pipelines(device: &wgpu::Device) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("edit lens"),
        source: wgpu::ShaderSource::Wgsl(include_str!("lens.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("edit lens"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("edit lens"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = |entry: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("edit lens"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: LENS_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let (weigh, halve) = (pipeline("weigh"), pipeline("halve"));
    (layout, weigh, halve)
}

/// What the preview samples of a recipe's paintings: a layer per painting, kept up to date with what changed.
pub(super) struct PaintTexture {
    texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    size: (u32, u32),
    /// What each layer shows, and its values.
    pub(super) drawn: Vec<(Painting, Vec<f32>)>,
}

impl PaintTexture {
    pub(super) fn new(device: &wgpu::Device, photo_size: [f32; 2]) -> Self {
        let size = domain::paint::grid_size(photo_size);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edit paintings"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: MAX_PAINTINGS as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let empty = vec![0.0; size.0 as usize * size.1 as usize];
        Self {
            texture,
            view,
            size,
            drawn: vec![(Painting::default(), empty); MAX_PAINTINGS],
        }
    }

    /// Draws what changed since the last time and uploads those layers. While the user paints, that's the stroke's new
    /// segments; a thinned stroke, an undo or a removed shape draws the layer anew.
    pub(super) fn sync(&mut self, queue: &wgpu::Queue, paintings: &[Painting]) {
        let (width, height) = self.size;
        let nothing = Painting::default();
        for layer in 0..MAX_PAINTINGS {
            let target = paintings.get(layer).unwrap_or(&nothing);
            let (shown, values) = &mut self.drawn[layer];
            if shown == target {
                continue;
            }
            match added_to(shown, target) {
                Some((stroke, from)) => domain::paint::draw_stroke(values, width, height, stroke, from),
                None => {
                    values.fill(0.0);
                    for stroke in &target.strokes {
                        domain::paint::draw_stroke(values, width, height, stroke, 0);
                    }
                }
            }
            shown.clone_from(target);
            let bytes: Vec<u8> = values
                .iter()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
                .collect();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

/// What `target` adds to `shown`, if that's all that changed: the stroke to draw and from which of its segments.
pub(super) fn added_to<'a>(shown: &Painting, target: &'a Painting) -> Option<(&'a Stroke, usize)> {
    let done = shown.strokes.len();
    match target.strokes.len().checked_sub(done)? {
        1 if target.strokes[..done] == shown.strokes[..] => Some((target.strokes.last()?, 0)),
        0 if done > 0 && target.strokes[..done - 1] == shown.strokes[..done - 1] => {
            let (before, now) = (&shown.strokes[done - 1], &target.strokes[done - 1]);
            let same = Stroke {
                points: now.points.get(..before.points.len())?.to_vec(),
                ..now.clone()
            };
            (same == *before && !now.closed).then_some((now, before.points.len()))
        }
        _ => None,
    }
}

/// A one-channel texture for a subject's mask, zeroed.
fn mask_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("subject"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The subject's mask as a texture the preview samples like the photo.
pub(super) fn upload_mask(render_state: &egui_wgpu::RenderState, mask: &Mask) -> wgpu::TextureView {
    let (width, height) = (mask.width(), mask.height());
    let texture = mask_texture(&render_state.device, width, height);
    let bytes: Vec<u8> = mask
        .values()
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    render_state.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Installs the preview's pipeline into the egui renderer. Call once at startup.
pub fn install(render_state: &egui_wgpu::RenderState) {
    let resources = EditResources::new(&render_state.device, render_state.target_format);
    render_state.renderer.write().callback_resources.insert(resources);
}

pub(super) struct PreviewCallback {
    pub(super) view: wgpu::TextureView,
    /// The subject's mask, if found.
    pub(super) mask: Option<wgpu::TextureView>,
    /// The recipe's paintings, if it has any.
    pub(super) paintings: Option<wgpu::TextureView>,
    /// The photo's lens pyramid, while a lens blur shows (REQ-023-05).
    pub(super) lens: Option<wgpu::TextureView>,
    /// The LUT's table, while the recipe has one this computer has (REQ-043-02).
    pub(super) lut: Option<wgpu::TextureView>,
    pub(super) uniform: Vec<f32>,
    /// Made in `prepare`; dropped with the frame, so the photo's texture isn't kept alive after editing ends.
    pub(super) group: std::sync::OnceLock<wgpu::BindGroup>,
}

impl CallbackTrait for PreviewCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(res) = resources.get::<EditResources>() {
            let _ = self.group.set(res.bind(
                device,
                &self.view,
                self.mask.as_ref(),
                self.paintings.as_ref(),
                self.lens.as_ref(),
                self.lut.as_ref(),
                &self.uniform,
            ));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(res) = resources.get::<EditResources>() else {
            return;
        };
        let Some(group) = self.group.get() else { return };
        pass.set_pipeline(&res.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}
