// Glass: dual-Kawase down / up passes.
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

