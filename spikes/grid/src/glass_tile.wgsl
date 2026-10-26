// Glass: tiles into the offscreen target.
struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

// ---- Tiles: one instance per tile, a triangle strip of 4 vertices. `rect` is NDC (x0, y0) top-left to (x1, y1).

@group(0) @binding(0) var tile_tex: texture_2d<f32>;
@group(0) @binding(1) var tile_sampler: sampler;

@vertex
fn vs_tile(@builtin(vertex_index) vi: u32, @location(0) rect: vec4f, @location(1) color: vec4f) -> VOut {
    let c = vec2f(f32(vi & 1u), f32(vi >> 1u));
    var out: VOut;
    out.pos = vec4f(mix(rect.xy, rect.zw, c), 0.0, 1.0);
    out.uv = c;
    out.color = color;
    return out;
}

@fragment
fn fs_tile(in: VOut) -> @location(0) vec4f {
    return textureSample(tile_tex, tile_sampler, in.uv) * in.color;
}

