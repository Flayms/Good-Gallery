//! Glass surfaces (REQ-013-03, `docs/design.md`): what lies under a surface is drawn into an offscreen target at half
//! resolution, blurred with a dual-Kawase chain and composited with saturation, tint, rounded corners and an opacity in
//! an `egui_wgpu` paint callback (≈ 0.4 ms per surface).
//!
//! What lies under glass is the frame's **backdrop**: the gallery records every tile it draws (its rect, its texture or
//! placeholder color) and the solid surfaces (page, sidebar); text isn't part of it, which a 24–32 px blur hides
//! anyway. Glass never blurs other glass. The viewer's chrome reuses the scrim's blur instead of blurring again.
//!
//! Without the pass (no wgpu render state, as in tests, or pipelines that couldn't be made) and with Windows'
//! _Transparency effects_ off (REQ-013-06), surfaces are opaque: Raised, Darkroom for the scrim.

use crate::theme::{self, Glass};
use domain::ambient::Rgb;
use eframe::egui::{self, Color32, CornerRadius, Rect, Shape, Stroke, StrokeKind};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Down and up passes after the half-resolution source: 4 give the design's 24–32 px blur.
const PASSES: u32 = 4;
/// Floats per backdrop instance: NDC rect, UV rect, color.
const INSTANCE_FLOATS: usize = 12;

/// Something under glass: a tile with its texture (or a placeholder color) or a solid surface.
#[derive(Clone)]
pub struct BackdropItem {
    pub rect: Rect,
    /// The part of the texture shown (tiles are cropped like `object-fit: cover`).
    pub uv: Rect,
    pub view: Option<wgpu::TextureView>,
    /// Multiplies the texture, premultiplied; for solids and placeholders, the color itself.
    pub color: Color32,
    /// Glass itself (the toolbar): only the scrim behind the viewer shows it, other glass never blurs glass.
    pub chrome: bool,
}

/// What this frame drew under where glass may go, bottom first.
#[derive(Clone, Default)]
struct Backdrop {
    items: Vec<BackdropItem>,
    /// Glass surfaces painted so far this frame; each has its own offscreen chain.
    surfaces: u32,
}

fn backdrop_id() -> egui::Id {
    egui::Id::unique("glass backdrop")
}

/// Makes `ctx` paint glass as if the blur pass were installed (tests have no GPU).
#[cfg(test)]
pub(crate) fn pretend_installed(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(installed_id(), true));
}

fn installed_id() -> egui::Id {
    egui::Id::unique("glass installed")
}

/// Starts a frame's backdrop. Call at the start of every frame.
pub fn begin_frame(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(backdrop_id(), Backdrop::default()));
}

/// Records a tile (or anything else drawn with a texture) under where glass may go.
pub fn record(ctx: &egui::Context, item: BackdropItem) {
    if !available(ctx) {
        return;
    }
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Backdrop>(backdrop_id()).items.push(item);
    });
}

/// Records a solid surface (the page, the sidebar), or with `chrome` a glass surface only the scrim shows.
pub fn record_solid(ctx: &egui::Context, rect: Rect, color: Color32, chrome: bool) {
    record(
        ctx,
        BackdropItem {
            rect,
            uv: Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            view: None,
            color,
            chrome,
        },
    );
}

/// Whether surfaces are translucent: the pass is installed and the OS doesn't ask for reduced transparency.
pub fn available(ctx: &egui::Context) -> bool {
    let installed = ctx.data(|data| data.get_temp::<bool>(installed_id())).unwrap_or(false);
    installed && !theme::reduced_transparency(ctx)
}

/// A glass surface: where, its token, the item's tint (REQ-013-11) and an opacity for fades.
#[derive(Debug, Clone, Copy)]
pub struct Surface {
    pub rect: Rect,
    pub radius: f32,
    pub glass: Glass,
    pub tint: Option<Rgb>,
    pub opacity: f32,
}

impl Surface {
    pub fn new(rect: Rect, radius: f32, glass: Glass) -> Self {
        Self {
            rect,
            radius,
            glass,
            tint: None,
            opacity: 1.0,
        }
    }

    pub fn tint(mut self, tint: Option<Rgb>) -> Self {
        self.tint = tint;
        self
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// The opaque surface used without glass: Raised (Darkroom for the scrim), tinted like the glass.
    fn opaque(&self) -> Shape {
        let fill = if self.glass == theme::SCRIM {
            theme::DARK.base
        } else {
            let base = Glass {
                color: rgb_of(theme::raised()),
                alpha: 1.0,
                ..self.glass
            };
            base.fill(self.tint)
        };
        Shape::rect_filled(self.rect, self.radius, fill.gamma_multiply(self.opacity))
    }
}

fn rgb_of(color: Color32) -> Rgb {
    Rgb {
        r: f32::from(color.r()),
        g: f32::from(color.g()),
        b: f32::from(color.b()),
    }
}

/// Where a surface's blurred backdrop comes from.
#[derive(Debug, Clone, Copy)]
pub enum Source {
    /// Its own blur of the frame's backdrop.
    Backdrop,
    /// The blur of an earlier surface, darkened by that surface's glass first (the viewer's chrome over the scrim).
    Over(Painted),
}

/// A surface painted with glass this frame, for [`Source::Over`].
#[derive(Debug, Clone, Copy)]
pub struct Painted {
    slot: u32,
    glass: Glass,
}

/// The shape for a surface: glass from `source`, or the opaque surface without glass. The `Painted` is `None` without
/// glass.
pub fn shape(ctx: &egui::Context, surface: Surface, source: Source) -> (Shape, Option<Painted>) {
    let screen = ctx.content_rect();
    let region = surface.rect.intersect(screen);
    if !available(ctx) || !region.is_positive() {
        return (surface.opaque(), None);
    }
    let slot = ctx.data_mut(|data| {
        let backdrop = data.get_temp_mut_or_default::<Backdrop>(backdrop_id());
        backdrop.surfaces += 1;
        backdrop.surfaces
    });
    let (items, over) = match source {
        Source::Backdrop => (Arc::new(under(ctx, region, surface.glass == theme::SCRIM)), None),
        Source::Over(painted) => (Arc::new(Vec::new()), Some(painted)),
    };
    let tint = surface.glass.fill(surface.tint);
    let callback = GlassCallback {
        slot,
        region,
        items,
        reuse: over.map(|painted| painted.slot),
        under: over.map_or([0.0; 4], |painted| unpremultiplied(painted.glass.fill(None))),
        tint: unpremultiplied(tint),
        saturate: surface.glass.saturate,
        radius: surface.radius,
        opacity: surface.opacity.clamp(0.0, 1.0),
        background: theme::palette_of(ctx.theme()).base,
    };
    let shape = egui_wgpu::Callback::new_paint_callback(region, callback).into();
    (
        shape,
        Some(Painted {
            slot,
            glass: surface.glass,
        }),
    )
}

/// The shape for `surface` as `painter` will paint it: the painter's opacity reaches ordinary shapes by itself, a
/// paint callback only through the surface's own opacity.
fn place(painter: &egui::Painter, surface: Surface, source: Source) -> (Shape, Option<Painted>) {
    let surface = if available(painter.ctx()) {
        surface.opacity(surface.opacity * painter.opacity())
    } else {
        surface
    };
    shape(painter.ctx(), surface, source)
}

/// What a surface over `region` blurs: the backdrop near it, without other glass unless it is the scrim.
fn under(ctx: &egui::Context, region: Rect, scrim: bool) -> Vec<BackdropItem> {
    let area = region.expand(margin());
    ctx.data(|data| {
        data.get_temp::<Backdrop>(backdrop_id())
            .map(|backdrop| {
                backdrop
                    .items
                    .iter()
                    .filter(|item| (scrim || !item.chrome) && item.rect.intersects(area))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Paints a surface with glass from the frame's backdrop (or opaque), returning it for [`Source::Over`].
pub fn paint(painter: &egui::Painter, surface: Surface) -> Option<Painted> {
    let (shape, painted) = place(painter, surface, Source::Backdrop);
    painter.add(shape);
    painted
}

/// The look of the viewer's chrome (REQ-013-11): a glass level tinted by the item, over the scrim's blur.
#[derive(Debug, Clone, Copy)]
pub struct Backing {
    pub glass: Glass,
    pub tint: Option<Rgb>,
    /// The scrim painted this frame, whose blur the chrome shows.
    pub over: Option<Painted>,
}

impl Backing {
    /// Paints the backing and its Rim into `rect`.
    pub fn paint(&self, painter: &egui::Painter, rect: Rect, radius: f32) {
        let source = self.over.map_or(Source::Backdrop, Source::Over);
        let surface = Surface::new(rect, radius, self.glass).tint(self.tint);
        painter.add(place(painter, surface, source).0);
        painter.add(rim(rect, radius));
    }

    /// The color the surface has without glass, translucent as when it had no blur (for tests).
    pub fn fill(&self) -> Color32 {
        self.glass.fill(self.tint)
    }
}

/// Lays out `add` in a Glass panel frame (menus, popovers, dialogs) with `margin` inside: glass (or Raised) and the Rim
/// behind the content. Two shape slots are kept before the content and filled after it, when its size is known.
pub fn panel<R>(ui: &mut egui::Ui, margin: egui::Margin, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let glass_slot = ui.painter().add(Shape::Noop);
    let rim_slot = ui.painter().add(Shape::Noop);
    theme::apply_style(ui);
    let inner = egui::Frame::new().inner_margin(margin).show(ui, add);
    let rect = inner.response.rect;
    let radius = f32::from(theme::RADIUS_PANEL);
    let (shape, _) = place(
        ui.painter(),
        Surface::new(rect, radius, theme::glass_panel()),
        Source::Backdrop,
    );
    ui.painter().set(glass_slot, shape);
    ui.painter().set(rim_slot, rim(rect, radius));
    inner.inner
}

fn rim(rect: Rect, radius: f32) -> Shape {
    egui::epaint::RectShape::stroke(
        rect,
        CornerRadius::same(radius as u8),
        Stroke::new(1.0, theme::rim()),
        StrokeKind::Inside,
    )
    .into()
}

/// Paints glass behind the content of one of egui's menus (popups, context menus, combo box lists), whose frame has
/// the style's menu margin around `add`.
pub fn behind_menu<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let margin = ui.style().spacing.menu_margin;
    behind_frame(ui, margin, add)
}

/// Paints glass and the Rim behind the content `add` lays out in an opaque frame with `margin` around it, covering the
/// frame's fill; two shape slots are kept before the content and filled after it, when its size is known. Without glass
/// the frame stays as it is, unless it comes from the other appearance (egui's global style, e.g. a menu in the dark
/// viewer in light, REQ-020-05): then the opaque surface of the current palette covers it. The content always gets the
/// current palette's style.
pub fn behind_frame<R>(ui: &mut egui::Ui, margin: egui::Margin, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let glass_slot = ui.painter().add(Shape::Noop);
    let rim_slot = ui.painter().add(Shape::Noop);
    theme::apply_style(ui);
    let inner = add(ui);
    let rect = ui.min_rect() + margin;
    let radius = f32::from(theme::RADIUS_PANEL);
    let (shape, _) = place(
        ui.painter(),
        Surface::new(rect, radius, theme::glass_panel()),
        Source::Backdrop,
    );
    if matches!(shape, Shape::Callback(_)) || ui.ctx().theme() != theme::palette().theme {
        ui.painter().set(glass_slot, shape);
        ui.painter().set(rim_slot, rim(rect, radius));
    }
    inner
}

/// Paints glass and the Rim behind an area that `add` shows with its own opaque frame, one that can't be framed from
/// outside (egui_extras' date picker popup, REQ-013-03). The frame is the first shape on the area's `layer`; it gives
/// way to its shadow, the glass (or Raised) and the Rim, in slots kept before `add`.
pub fn behind_area<R>(ctx: &egui::Context, layer: egui::LayerId, add: impl FnOnce() -> R) -> R {
    let painter = ctx.layer_painter(layer);
    let shadow_slot = painter.add(Shape::Noop);
    let glass_slot = painter.add(Shape::Noop);
    let frame_slot = egui::layers::ShapeIdx(glass_slot.0 + 1);
    let inner = add();
    let shown = ctx.graphics(|graphics| graphics.get(layer).is_some_and(|list| list.next_idx().0 > frame_slot.0));
    if shown && let Some(rect) = ctx.memory(|memory| memory.area_rect(layer.id)) {
        let radius = f32::from(theme::RADIUS_PANEL);
        let (glass, _) = place(
            &painter,
            Surface::new(rect, radius, theme::glass_panel()),
            Source::Backdrop,
        );
        let shadow = ctx
            .global_style()
            .visuals
            .popup_shadow
            .as_shape(rect, theme::RADIUS_PANEL);
        painter.set(shadow_slot, shadow);
        painter.set(glass_slot, glass);
        painter.set(frame_slot, rim(rect, radius));
    }
    inner
}

/// Margin around a surface that is rendered too, so the blur doesn't fade in from its edges.
fn margin() -> f32 {
    4.0 * (1u32 << PASSES) as f32
}

fn unpremultiplied(color: Color32) -> [f32; 4] {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    [r, g, b, a].map(|c| f32::from(c) / 255.0)
}

fn floats_to_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_ne_bytes()).collect()
}

/// Installs the pipelines into the egui renderer; glass is used from then on. Call once at startup.
pub fn install(ctx: &egui::Context, render_state: &egui_wgpu::RenderState) {
    let resources = GlassResources::new(render_state);
    render_state.renderer.write().callback_resources.insert(resources);
    ctx.data_mut(|data| data.insert_temp(installed_id(), true));
}

struct Slot {
    sizes: Vec<[u32; 2]>,
    views: Vec<wgpu::TextureView>,
    /// The screen rect (points) level 0 covers.
    source: Rect,
    composite: Option<wgpu::BindGroup>,
}

struct GlassResources {
    tile: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    tile_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    slots: HashMap<u32, Slot>,
}

fn bind_group_layout(device: &wgpu::Device, uniform: bool) -> wgpu::BindGroupLayout {
    let mut entries = vec![
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
    ];
    if uniform {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("glass"),
        entries: &entries,
    })
}

struct PipelineSpec<'a> {
    vs: &'a str,
    fs: &'a str,
    layout: &'a wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    instances: bool,
    blend: Option<wgpu::BlendState>,
}

fn pipeline(device: &wgpu::Device, module: &wgpu::ShaderModule, spec: PipelineSpec) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(spec.fs),
        bind_group_layouts: &[Some(spec.layout)],
        immediate_size: 0,
    });
    let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: (INSTANCE_FLOATS * size_of::<f32>()) as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &attributes,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.fs),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(spec.vs),
            compilation_options: Default::default(),
            buffers: if spec.instances { &buffers } else { &[] },
        },
        primitive: wgpu::PrimitiveState {
            topology: if spec.instances {
                wgpu::PrimitiveTopology::TriangleStrip
            } else {
                wgpu::PrimitiveTopology::TriangleList
            },
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(spec.fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: spec.format,
                blend: spec.blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl GlassResources {
    fn new(render_state: &egui_wgpu::RenderState) -> Self {
        let device = &render_state.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glass.wgsl").into()),
        });
        let tile_layout = bind_group_layout(device, false);
        let blur_layout = bind_group_layout(device, true);
        let white = device
            .create_texture_with_data(
                &render_state.queue,
                &wgpu::TextureDescriptor {
                    label: Some("glass white"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[255, 255, 255, 255],
            )
            .create_view(&Default::default());
        let premultiplied = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let spec = |vs, fs, layout, format, instances, blend| PipelineSpec {
            vs,
            fs,
            layout,
            format,
            instances,
            blend,
        };
        Self {
            tile: pipeline(
                device,
                &module,
                spec("vs_tile", "fs_tile", &tile_layout, FORMAT, true, premultiplied),
            ),
            down: pipeline(
                device,
                &module,
                spec("vs_full", "fs_down", &blur_layout, FORMAT, false, None),
            ),
            up: pipeline(
                device,
                &module,
                spec("vs_full", "fs_up", &blur_layout, FORMAT, false, None),
            ),
            composite: pipeline(
                device,
                &module,
                spec(
                    "vs_full",
                    "fs_composite",
                    &blur_layout,
                    render_state.target_format,
                    false,
                    premultiplied,
                ),
            ),
            tile_layout,
            blur_layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("glass"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            white,
            slots: HashMap::new(),
        }
    }

    fn bind_group(&self, device: &wgpu::Device, view: &wgpu::TextureView, uniform: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass"),
            layout: &self.blur_layout,
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
                    resource: uniform.as_entire_binding(),
                },
            ],
        })
    }
}

struct GlassCallback {
    slot: u32,
    region: Rect,
    items: Arc<Vec<BackdropItem>>,
    /// The slot whose blur this surface shows instead of its own.
    reuse: Option<u32>,
    /// The reused surface's glass, laid over the blur first (unpremultiplied).
    under: [f32; 4],
    tint: [f32; 4],
    saturate: f32,
    radius: f32,
    opacity: f32,
    /// The page behind the recorded backdrop: the window's own appearance, also under the dark viewer's scrim.
    background: Color32,
}

fn level_view(device: &wgpu::Device, [width, height]: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("glass level"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    clear: wgpu::Color,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("glass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

impl GlassCallback {
    /// Draws the backdrop into level 0 of the slot and blurs it.
    fn blur(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        res: &mut GlassResources,
        pixels_per_point: f32,
    ) {
        let source = self.region.expand(margin());
        let level0 = [
            ((source.width() * pixels_per_point / 2.0).ceil() as u32).max(1),
            ((source.height() * pixels_per_point / 2.0).ceil() as u32).max(1),
        ];
        let mut sizes = vec![level0];
        for i in 0..PASSES as usize {
            let [w, h] = sizes[i];
            sizes.push([(w / 2).max(1), (h / 2).max(1)]);
        }
        let slot = res.slots.entry(self.slot).or_insert_with(|| Slot {
            sizes: Vec::new(),
            views: Vec::new(),
            source,
            composite: None,
        });
        if slot.sizes != sizes {
            slot.views = sizes.iter().map(|&size| level_view(device, size)).collect();
            slot.sizes = sizes.clone();
        }
        slot.source = source;

        let to_ndc = |p: egui::Pos2| {
            [
                (p.x - source.left()) / source.width() * 2.0 - 1.0,
                1.0 - (p.y - source.top()) / source.height() * 2.0,
            ]
        };
        let mut instances = Vec::with_capacity(self.items.len() * INSTANCE_FLOATS);
        for item in self.items.iter() {
            let [x0, y0] = to_ndc(item.rect.min);
            let [x1, y1] = to_ndc(item.rect.max);
            let color = item.color.to_array().map(|c| f32::from(c) / 255.0);
            instances.extend_from_slice(&[
                x0,
                y0,
                x1,
                y1,
                item.uv.min.x,
                item.uv.min.y,
                item.uv.max.x,
                item.uv.max.y,
            ]);
            instances.extend_from_slice(&color);
        }
        let groups: Vec<wgpu::BindGroup> = self
            .items
            .iter()
            .map(|item| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glass tile"),
                    layout: &res.tile_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(item.view.as_ref().unwrap_or(&res.white)),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&res.sampler),
                        },
                    ],
                })
            })
            .collect();
        let buffer = (!instances.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glass backdrop"),
                contents: &floats_to_bytes(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        let [r, g, b, _] = self.background.to_normalized_gamma_f32();
        let background = wgpu::Color {
            r: f64::from(r),
            g: f64::from(g),
            b: f64::from(b),
            a: 1.0,
        };
        {
            let mut pass = pass(encoder, &slot.views[0], background);
            if let Some(buffer) = &buffer {
                pass.set_pipeline(&res.tile);
                pass.set_vertex_buffer(0, buffer.slice(..));
                for (i, group) in groups.iter().enumerate() {
                    pass.set_bind_group(0, group, &[]);
                    pass.draw(0..4, i as u32..i as u32 + 1);
                }
            }
        }

        // Dual Kawase: down the chain, then back up into level 0.
        let steps = (0..PASSES as usize)
            .map(|i| (true, i, i + 1))
            .chain((0..PASSES as usize).rev().map(|i| (false, i + 1, i)));
        for (down, from, to) in steps {
            let [w, h] = sizes[from];
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glass blur"),
                contents: &floats_to_bytes(&[0.5 / w as f32, 0.5 / h as f32, 1.0, 0.0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let slot = &res.slots[&self.slot];
            let group = res.bind_group(device, &slot.views[from], &uniform);
            let mut pass = pass(encoder, &slot.views[to], wgpu::Color::BLACK);
            pass.set_pipeline(if down { &res.down } else { &res.up });
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

impl CallbackTrait for GlassCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(res) = resources.get_mut::<GlassResources>() else {
            return Vec::new();
        };
        let ppp = screen.pixels_per_point;
        let blurred_by = match self.reuse {
            None => {
                self.blur(device, encoder, res, ppp);
                self.slot
            }
            Some(slot) => slot,
        };
        let Some(source) = res.slots.get(&blurred_by).map(|slot| slot.source) else {
            return Vec::new();
        };
        let uv = [
            (self.region.left() - source.left()) / source.width(),
            (self.region.top() - source.top()) / source.height(),
            (self.region.right() - source.left()) / source.width(),
            (self.region.bottom() - source.top()) / source.height(),
        ];
        let mut uniform = Vec::with_capacity(20);
        uniform.extend_from_slice(&uv);
        uniform.extend_from_slice(&self.under);
        uniform.extend_from_slice(&self.tint);
        uniform.extend_from_slice(&[
            self.region.width() * ppp,
            self.region.height() * ppp,
            self.radius * ppp,
            self.saturate,
            self.opacity,
            0.0,
            0.0,
            0.0,
        ]);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("glass composite"),
            contents: &floats_to_bytes(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = res.bind_group(device, &res.slots[&blurred_by].views[0], &buffer);
        // A reused slot keeps its own composite; this surface's goes under its own number.
        let own = res.slots.entry(self.slot).or_insert_with(|| Slot {
            sizes: Vec::new(),
            views: Vec::new(),
            source,
            composite: None,
        });
        own.composite = Some(group);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(res) = resources.get::<GlassResources>() else {
            return;
        };
        if let Some(group) = res.slots.get(&self.slot).and_then(|slot| slot.composite.as_ref()) {
            pass.set_pipeline(&res.composite);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    fn ctx(installed: bool) -> egui::Context {
        let ctx = egui::Context::default();
        ctx.data_mut(|data| data.insert_temp(installed_id(), installed));
        begin_frame(&ctx);
        ctx
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn req_013_03_surfaces_blur_the_tiles_under_them_but_not_other_glass() {
        let ctx = ctx(true);
        let tile = |r: Rect| BackdropItem {
            rect: r,
            uv: rect(0.0, 0.0, 1.0, 1.0),
            view: None,
            color: Color32::WHITE,
            chrome: false,
        };
        record(&ctx, tile(rect(0.0, 20.0, 100.0, 100.0)));
        record(&ctx, tile(rect(0.0, 2000.0, 100.0, 100.0)));
        record_solid(&ctx, rect(0.0, 0.0, 800.0, 48.0), theme::glass_bar().fill(None), true);

        let toolbar = rect(0.0, 0.0, 800.0, 48.0);
        let under_bar = under(&ctx, toolbar, false);
        assert_eq!(
            under_bar.len(),
            1,
            "only the tile near the toolbar, not the toolbar itself"
        );
        assert_eq!(under_bar[0].rect, rect(0.0, 20.0, 100.0, 100.0));
        // The scrim behind the viewer shows the whole window, toolbar included.
        assert_eq!(under(&ctx, rect(0.0, 0.0, 800.0, 600.0), true).len(), 2);

        let (shape, painted) = shape(&ctx, Surface::new(toolbar, 0.0, theme::glass_bar()), Source::Backdrop);
        assert!(matches!(shape, Shape::Callback(_)));
        assert!(painted.is_some());
    }

    /// REQ-020-06: in the light appearance glass becomes the light Raised; the scrim stays dark (REQ-020-05).
    #[test]
    fn req_020_06_light_surfaces_are_opaque_without_glass() {
        let fill = |shape: Shape| match shape {
            Shape::Rect(rect) => rect.fill,
            other => panic!("not opaque: {other:?}"),
        };
        let reduced = ctx(true);
        theme::set_reduced_transparency(&reduced, true);
        theme::with_palette(&theme::LIGHT, || {
            for glass in [theme::glass_bar(), theme::glass_panel()] {
                let surface = Surface::new(rect(0.0, 0.0, 100.0, 48.0), 0.0, glass);
                assert_eq!(fill(shape(&reduced, surface, Source::Backdrop).0), theme::LIGHT.raised);
            }
            let scrim = Surface::new(rect(0.0, 0.0, 800.0, 600.0), 0.0, theme::SCRIM);
            assert_eq!(fill(shape(&reduced, scrim, Source::Backdrop).0), theme::DARK.base);
        });
    }

    #[test]
    fn req_013_06_req_013_14_surfaces_are_opaque_without_glass() {
        let raised = |shape: Shape| match shape {
            Shape::Rect(rect) => rect.fill,
            other => panic!("not opaque: {other:?}"),
        };
        let bar = Surface::new(rect(0.0, 0.0, 100.0, 48.0), 0.0, theme::glass_bar());
        let scrim = Surface::new(rect(0.0, 0.0, 800.0, 600.0), 0.0, theme::SCRIM);

        // Reduced transparency.
        let reduced = ctx(true);
        theme::set_reduced_transparency(&reduced, true);
        assert!(!available(&reduced));
        assert_eq!(raised(shape(&reduced, bar, Source::Backdrop).0), theme::raised());
        assert_eq!(raised(shape(&reduced, scrim, Source::Backdrop).0), theme::DARK.base);
        // No blur pass (no GPU, or tests).
        let none = ctx(false);
        assert_eq!(raised(shape(&none, bar, Source::Backdrop).0), theme::raised());
        assert_eq!(shape(&none, bar, Source::Backdrop).1.map(|p| p.slot), None);
        // The tint stays on the opaque surface (REQ-013-11).
        let tint = Rgb {
            r: 200.0,
            g: 0.0,
            b: 0.0,
        };
        let tinted = raised(shape(&none, bar.tint(Some(tint)), Source::Backdrop).0);
        assert!(tinted.r() > tinted.g() && tinted.a() == 255, "{tinted:?}");

        theme::set_reduced_transparency(&reduced, false);
        assert!(available(&reduced));
    }
}
