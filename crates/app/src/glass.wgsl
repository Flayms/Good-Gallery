// Glass (docs/design.md): the backdrop under a surface is drawn into an offscreen target at half resolution, blurred
// with a dual-Kawase chain and composited with saturation, tint, rounded corners and an opacity. Ported from the M0
// spike (`spikes/grid/src/glass_*.wgsl`).

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

// ---- Backdrop: one instance per tile or solid, a triangle strip of 4 vertices. `rect` is NDC (x0, y0) top-left to
// (x1, y1); `uv` the texture's part; `color` multiplies the texture (premultiplied).

@group(0) @binding(0) var tile_tex: texture_2d<f32>;
@group(0) @binding(1) var tile_sampler: sampler;

@vertex
fn vs_tile(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4f,
    @location(1) uv: vec4f,
    @location(2) color: vec4f,
) -> VOut {
    let c = vec2f(f32(vi & 1u), f32(vi >> 1u));
    var out: VOut;
    out.pos = vec4f(mix(rect.xy, rect.zw, c), 0.0, 1.0);
    out.uv = mix(uv.xy, uv.zw, c);
    out.color = color;
    return out;
}

@fragment
fn fs_tile(in: VOut) -> @location(0) vec4f {
    return textureSample(tile_tex, tile_sampler, in.uv) * in.color;
}

// ---- Full-screen triangle for the blur and composite passes.

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> VOut {
    let p = vec2f(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: VOut;
    out.pos = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2f(p.x, 1.0 - p.y);
    out.color = vec4f(1.0);
    return out;
}

struct Blur {
    half_pixel: vec2f,
    offset: f32,
    _pad: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> blur: Blur;

@fragment
fn fs_down(in: VOut) -> @location(0) vec4f {
    let hp = blur.half_pixel * blur.offset;
    var sum = textureSample(src, src_sampler, in.uv) * 4.0;
    sum += textureSample(src, src_sampler, in.uv - hp);
    sum += textureSample(src, src_sampler, in.uv + hp);
    sum += textureSample(src, src_sampler, in.uv + vec2f(hp.x, -hp.y));
    sum += textureSample(src, src_sampler, in.uv - vec2f(hp.x, -hp.y));
    return sum / 8.0;
}

@fragment
fn fs_up(in: VOut) -> @location(0) vec4f {
    let hp = blur.half_pixel * blur.offset;
    var sum = textureSample(src, src_sampler, in.uv + vec2f(-hp.x * 2.0, 0.0));
    sum += textureSample(src, src_sampler, in.uv + vec2f(-hp.x, hp.y)) * 2.0;
    sum += textureSample(src, src_sampler, in.uv + vec2f(0.0, hp.y * 2.0));
    sum += textureSample(src, src_sampler, in.uv + vec2f(hp.x, hp.y)) * 2.0;
    sum += textureSample(src, src_sampler, in.uv + vec2f(hp.x * 2.0, 0.0));
    sum += textureSample(src, src_sampler, in.uv + vec2f(hp.x, -hp.y)) * 2.0;
    sum += textureSample(src, src_sampler, in.uv + vec2f(0.0, -hp.y * 2.0));
    sum += textureSample(src, src_sampler, in.uv + vec2f(-hp.x, -hp.y)) * 2.0;
    return sum / 12.0;
}

// ---- Composite into egui's pass, inside the callback's viewport.

struct Composite {
    /// Region of the blurred texture to show: u0, v0, u1, v1.
    uv_rect: vec4f,
    /// A first layer over the blur (the scrim under the viewer's chrome), color and opacity.
    under: vec4f,
    /// Tint color (gamma space) and its opacity.
    tint: vec4f,
    size_px: vec2f,
    radius_px: f32,
    saturate: f32,
    opacity: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(2) var<uniform> comp: Composite;

@fragment
fn fs_composite(in: VOut) -> @location(0) vec4f {
    let uv = mix(comp.uv_rect.xy, comp.uv_rect.zw, in.uv);
    var c = textureSample(src, src_sampler, uv).rgb;
    let luma = dot(c, vec3f(0.2126, 0.7152, 0.0722));
    c = clamp(mix(vec3f(luma), c, comp.saturate), vec3f(0.0), vec3f(1.0));
    c = mix(c, comp.under.rgb, comp.under.a);
    c = mix(c, comp.tint.rgb, comp.tint.a);
    // Rounded-rect coverage from a signed distance, 1 px antialiasing.
    let p = in.uv * comp.size_px;
    let half_size = comp.size_px * 0.5;
    let q = abs(p - half_size) - (half_size - vec2f(comp.radius_px));
    let d = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - comp.radius_px;
    let a = clamp(0.5 - d, 0.0, 1.0) * comp.opacity;
    return vec4f(c * a, a);
}
