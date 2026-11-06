// The lens blur's pyramid (REQ-023-05): the photo's pixels times `domain::edit::lens_weight`, with the weight, halved
// again and again as `domain::edit::Pyramid` halves. Its level 0 is the photo's level 1: a disc never samples finer.

@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole viewport.
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

// `domain::edit::lens_weighted`.
fn weighted(c: vec4<f32>) -> vec4<f32> {
    let t = clamp((max(max(c.r, c.g), c.b) - 0.7) / 0.3, 0.0, 1.0);
    let w = 1.0 + 40.0 * t * t;
    return vec4<f32>(c.rgb * w, w);
}

// The photo's 2 × 2 pixels under this one, weighted, averaged.
@fragment
fn weigh(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(p.xy)) * 2;
    let a = weighted(textureLoad(source, at, 0));
    let b = weighted(textureLoad(source, at + vec2<i32>(1, 0), 0));
    let c = weighted(textureLoad(source, at + vec2<i32>(0, 1), 0));
    let d = weighted(textureLoad(source, at + vec2<i32>(1, 1), 0));
    return (a + b + c + d) / 4.0;
}

// The level above's 2 × 2 pixels under this one, averaged.
@fragment
fn halve(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(p.xy)) * 2;
    let a = textureLoad(source, at, 0);
    let b = textureLoad(source, at + vec2<i32>(1, 0), 0);
    let c = textureLoad(source, at + vec2<i32>(0, 1), 0);
    let d = textureLoad(source, at + vec2<i32>(1, 1), 0);
    return (a + b + c + d) / 4.0;
}
