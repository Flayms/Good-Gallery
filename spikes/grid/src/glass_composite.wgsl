// Glass: composite with saturate, tint and rounded corners.
struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

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

// ---- Composite into egui's pass, inside the callback's viewport.

struct Composite {
    /// Region of the blurred texture to show: u0, v0, u1, v1.
    uv_rect: vec4f,
    /// Tint color (gamma space) and its opacity.
    tint: vec4f,
    size_px: vec2f,
    radius_px: f32,
    saturate: f32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> comp: Composite;

@fragment
fn fs_composite(in: VOut) -> @location(0) vec4f {
    let uv = mix(comp.uv_rect.xy, comp.uv_rect.zw, in.uv);
    var c = textureSample(src, src_sampler, uv).rgb;
    let luma = dot(c, vec3f(0.2126, 0.7152, 0.0722));
    c = clamp(mix(vec3f(luma), c, comp.saturate), vec3f(0.0), vec3f(1.0));
    c = mix(c, comp.tint.rgb, comp.tint.a);
    // Rounded-rect coverage from a signed distance, 1 px antialiasing.
    let p = in.uv * comp.size_px;
    let half_size = comp.size_px * 0.5;
    let q = abs(p - half_size) - (half_size - vec2f(comp.radius_px));
    let d = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - comp.radius_px;
    let a = clamp(0.5 - d, 0.0, 1.0);
    return vec4f(c * a, a);
}
