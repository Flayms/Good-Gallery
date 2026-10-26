//! Glass surfaces over the gallery: the tiles under a region are drawn into an offscreen target at half
//! resolution, blurred with a dual-Kawase chain and composited with saturate, tint and rounded corners.

use eframe::egui::{self, Rect};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct TileInstance {
    rect: [f32; 4],
    color: [f32; 4],
}

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct BlurUniform {
    half_pixel: [f32; 2],
    offset: f32,
    _pad: f32,
}

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct CompositeUniform {
    uv_rect: [f32; 4],
    tint: [f32; 4],
    size_px: [f32; 2],
    radius_px: f32,
    saturate: f32,
}

struct Slot {
    sizes: Vec<[u32; 2]>,
    views: Vec<wgpu::TextureView>,
    composite: Option<wgpu::BindGroup>,
}

pub struct GlassResources {
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

fn layout(device: &wgpu::Device, uniform: bool) -> wgpu::BindGroupLayout {
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

#[allow(clippy::too_many_arguments)]
fn pipeline(
    device: &wgpu::Device,
    shader: &str,
    vs: &str,
    fs: &str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    instances: bool,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(fs),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(fs),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
    let buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<TileInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &attrs,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers: if instances { &buffers } else { &[] },
        },
        primitive: wgpu::PrimitiveState {
            topology: if instances {
                wgpu::PrimitiveTopology::TriangleStrip
            } else {
                wgpu::PrimitiveTopology::TriangleList
            },
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl GlassResources {
    pub fn install(rs: &egui_wgpu::RenderState) {
        let device = &rs.device;
        let tile_layout = layout(device, false);
        let blur_layout = layout(device, true);
        let white_tex = device.create_texture_with_data(
            &rs.queue,
            &wgpu::TextureDescriptor {
                label: Some("white"),
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
        );
        let res = Self {
            tile: pipeline(
                device,
                include_str!("glass_tile.wgsl"),
                "vs_tile",
                "fs_tile",
                &tile_layout,
                FORMAT,
                true,
                None,
            ),
            down: pipeline(
                device,
                include_str!("glass_blur.wgsl"),
                "vs_full",
                "fs_down",
                &blur_layout,
                FORMAT,
                false,
                None,
            ),
            up: pipeline(
                device,
                include_str!("glass_blur.wgsl"),
                "vs_full",
                "fs_up",
                &blur_layout,
                FORMAT,
                false,
                None,
            ),
            composite: pipeline(
                device,
                include_str!("glass_composite.wgsl"),
                "vs_full",
                "fs_composite",
                &blur_layout,
                rs.target_format,
                false,
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
            white: white_tex.create_view(&Default::default()),
            slots: HashMap::new(),
        };
        rs.renderer.write().callback_resources.insert(res);
    }
}

/// A tile under a glass region: its rect in points and its texture, or `None` for a placeholder.
pub struct GlassTile {
    pub rect: Rect,
    pub view: Option<wgpu::TextureView>,
}

pub struct Glass {
    /// One offscreen chain per glass surface.
    pub slot: u32,
    pub region: Rect,
    pub tiles: Vec<GlassTile>,
    /// Color of the gallery background behind the tiles.
    pub background: [f32; 4],
    pub placeholder: [f32; 4],
    /// Down / up passes after the half-resolution source; more passes blur wider.
    pub passes: u32,
    pub tint: [f32; 4],
    pub saturate: f32,
    pub radius: f32,
}

impl Glass {
    /// Margin around the region that is rendered too, so the blur doesn't fade in from the edges.
    fn margin(&self) -> f32 {
        4.0 * (1u32 << self.passes) as f32
    }

    pub fn paint(self, painter: &egui::Painter) {
        let rect = self.region;
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, self));
    }
}

impl CallbackTrait for Glass {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let res: &mut GlassResources = resources.get_mut().unwrap();
        let ppp = screen.pixels_per_point;
        let source = self.region.expand(self.margin());
        let level0 = [
            ((source.width() * ppp / 2.0).ceil() as u32).max(1),
            ((source.height() * ppp / 2.0).ceil() as u32).max(1),
        ];
        let passes = self.passes as usize;

        // Offscreen chain: level 0 at half resolution, then halving per pass.
        let slot = res.slots.entry(self.slot).or_insert_with(|| Slot {
            sizes: Vec::new(),
            views: Vec::new(),
            composite: None,
        });
        let mut sizes = vec![level0];
        for i in 0..passes {
            let [w, h] = sizes[i];
            sizes.push([(w / 2).max(1), (h / 2).max(1)]);
        }
        if slot.sizes != sizes {
            slot.views = sizes
                .iter()
                .map(|&[width, height]| {
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
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                })
                .collect();
            slot.sizes = sizes.clone();
        }

        // Tiles into level 0.
        let to_ndc = |x: f32, y: f32| {
            [
                (x - source.left()) / source.width() * 2.0 - 1.0,
                1.0 - (y - source.top()) / source.height() * 2.0,
            ]
        };
        let instances: Vec<TileInstance> = self
            .tiles
            .iter()
            .map(|t| {
                let [x0, y0] = to_ndc(t.rect.left(), t.rect.top());
                let [x1, y1] = to_ndc(t.rect.right(), t.rect.bottom());
                TileInstance {
                    rect: [x0, y0, x1, y1],
                    color: if t.view.is_some() {
                        [1.0; 4]
                    } else {
                        self.placeholder
                    },
                }
            })
            .collect();
        let tile_groups: Vec<wgpu::BindGroup> = self
            .tiles
            .iter()
            .map(|t| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glass tile"),
                    layout: &res.tile_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(
                                t.view.as_ref().unwrap_or(&res.white),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&res.sampler),
                        },
                    ],
                })
            })
            .collect();
        let instance_buf = (!instances.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glass tiles"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        let [r, g, b, a] = self.background;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass tiles"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &slot.views[0],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: r as f64,
                            g: g as f64,
                            b: b as f64,
                            a: a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(buf) = &instance_buf {
                pass.set_pipeline(&res.tile);
                pass.set_vertex_buffer(0, buf.slice(..));
                for (i, group) in tile_groups.iter().enumerate() {
                    pass.set_bind_group(0, group, &[]);
                    pass.draw(0..4, i as u32..i as u32 + 1);
                }
            }
        }

        // Dual Kawase: down through the chain, then back up into level 0.
        let blur_group = |src: usize, uniform: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass blur"),
                layout: &res.blur_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&slot.views[src]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&res.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            })
        };
        let mut run = |pipeline: &wgpu::RenderPipeline, src: usize, dst: usize| {
            let [w, h] = sizes[src];
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glass blur"),
                contents: bytemuck::bytes_of(&BlurUniform {
                    half_pixel: [0.5 / w as f32, 0.5 / h as f32],
                    offset: 1.0,
                    _pad: 0.0,
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = blur_group(src, &uniform);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &slot.views[dst],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
        };
        for i in 0..passes {
            run(&res.down, i, i + 1);
        }
        for i in (0..passes).rev() {
            run(&res.up, i + 1, i);
        }

        // Composite bind group: the region's part of level 0.
        let m = self.margin();
        let uv_rect = [
            m / source.width(),
            m / source.height(),
            1.0 - m / source.width(),
            1.0 - m / source.height(),
        ];
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("glass composite"),
            contents: bytemuck::bytes_of(&CompositeUniform {
                uv_rect,
                tint: self.tint,
                size_px: [self.region.width() * ppp, self.region.height() * ppp],
                radius_px: self.radius * ppp,
                saturate: self.saturate,
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        slot.composite = Some(blur_group(0, &uniform));
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let res: &GlassResources = resources.get().unwrap();
        if let Some(group) = res.slots.get(&self.slot).and_then(|s| s.composite.as_ref()) {
            pass.set_pipeline(&res.composite);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
