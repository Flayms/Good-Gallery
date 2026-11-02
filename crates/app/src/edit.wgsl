// The edit preview (spec 021): the unedited original drawn through the recipe. The formulas mirror
// `domain::edit::Params::pixel_masked`; a test compares both.

// The numbers of one part's light and color (spec 022), as `Edit`'s first ones.
struct Part {
    // White balance gains times exposure (xyz), the part has any (w).
    gains: vec4<f32>,
    tone: vec4<f32>,
    color: vec4<f32>,
    turn: vec4<f32>,
}

// A shape of a mask (spec 030): its kind (x: 0 gradient, 1 luminance, 2 color, 3 subject, 4 background, 5 painted), how it combines
// (y: 0 add, 1 subtract, 2 intersect), and its numbers (`domain::edit::MaskShape::numbers`).
struct MaskShape {
    kind: vec4<f32>,
    numbers: vec4<f32>,
}

// A mask: its light and color, inverted (info.x), how many shapes (info.y), and those.
struct LocalMask {
    part: Part,
    info: vec4<f32>,
    shapes: array<MaskShape, 4>,
}

struct Edit {
    // The photo's rect in framebuffer pixels: min (xy), max (zw).
    rect: vec4<f32>,
    // Texture coordinates at the rect's min (xy) and max (zw).
    uv: vec4<f32>,
    // The photo's full size in pixels (xy), opacity (z), flags (w): 1 = the original (compare), 2 = linear output,
    // 4 = the parts by the mask, 8 = tint the selection, 16 = the selection is the background.
    size: vec4<f32>,
    // White balance gains times exposure (xyz), curves on (w).
    gains: vec4<f32>,
    // Black point, white point, shadows, highlights.
    tone: vec4<f32>,
    // Contrast, saturation, vibrance, brightness (the midtones' gamma).
    color: vec4<f32>,
    // The hue's turn around the color wheel: its cosine (x) and sine (y).
    turn: vec4<f32>,
    // Vignette, vignette start, grain, grain cell size.
    fx: vec4<f32>,
    subject: Part,
    background: Part,
    // Blur (spec 023): the strongest as σ in parts of the longer side (x), the kind (y: 0 none, 1 background,
    // 2 tilt-shift, 3 radial), half the sharp area's size (z) and the transition (w), in parts of the longer side.
    blur: vec4<f32>,
    // The focus (xy), 0–1 across the photo, and the quarter turns clockwise (z).
    focus: vec4<f32>,
    // The crop on the upright, unturned photo: x, y, w, h (spec 029).
    crop: vec4<f32>,
    // A film look's grain (REQ-028-06): strongest in the midtones (x), how much it differs per channel (y); its change
    // per hue (z: on, REQ-028-07) and its halation (w, REQ-028-08).
    film: vec4<f32>,
    // Around the color wheel: the turn in radians (x), chroma's factor (y), the lightness added (z).
    hue_nodes: array<vec4<f32>, 8>,
    // How many masks there are (x).
    mask_count: vec4<f32>,
    masks: array<LocalMask, 8>,
    curves: array<vec4<f32>, 256>,
}

@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var photo_sampler: sampler;
@group(0) @binding(2) var<uniform> edit: Edit;
// How much each spot belongs to the subject (red); 1 × 1 while there is none.
@group(0) @binding(3) var subject_mask: texture_2d<f32>;
// What the recipe's paintings select (spec 030), one layer each.
@group(0) @binding(4) var paintings: texture_2d_array<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole viewport.
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn to_encoded(c: f32) -> f32 {
    if c <= 0.0031308 {
        return c * 12.92;
    }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn luminance(c: vec3<f32>) -> f32 {
    return 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
}

fn tone(value: f32, p: Part) -> f32 {
    var l = (value - p.tone.x) / (p.tone.y - p.tone.x);
    if p.color.w != 1.0 && l > 0.0 && l < 1.0 {
        l = pow(l, p.color.w);
    }
    let t = clamp(l, 0.0, 1.0);
    l += p.tone.z * 6.75 * t * (1.0 - t) * (1.0 - t);
    l += p.tone.w * 6.75 * (1.0 - t) * t * t;
    let contrast = p.color.x;
    if contrast >= 0.0 {
        let x = clamp(l, 0.0, 1.0);
        return l + contrast * (x * x * (3.0 - 2.0 * x) - x);
    }
    return l + contrast * 0.5 * (l - 0.5);
}

fn lookup(channel: u32, x: f32) -> f32 {
    let at = clamp(x, 0.0, 1.0) * 255.0;
    let i = min(u32(floor(at)), 254u);
    let t = at - f32(i);
    return mix(edit.curves[i][channel], edit.curves[i + 1u][channel], t);
}

fn to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.41222146 * c.r + 0.53633255 * c.g + 0.051445995 * c.b;
    let m = 0.2119035 * c.r + 0.6806995 * c.g + 0.10739696 * c.b;
    let s = 0.08830246 * c.r + 0.28171885 * c.g + 0.6299787 * c.b;
    let lms = vec3<f32>(cbrt(l), cbrt(m), cbrt(s));
    return vec3<f32>(
        0.21045426 * lms.x + 0.7936178 * lms.y - 0.004072047 * lms.z,
        1.9779985 * lms.x - 2.4285922 * lms.y + 0.4505937 * lms.z,
        0.025904037 * lms.x + 0.78277177 * lms.y - 0.80867577 * lms.z,
    );
}

fn cbrt(x: f32) -> f32 {
    return sign(x) * pow(abs(x), 1.0 / 3.0);
}

fn from_oklab(c: vec3<f32>) -> vec3<f32> {
    let l_ = c.x + 0.39633778 * c.y + 0.21580376 * c.z;
    let m_ = c.x - 0.105561346 * c.y - 0.06385417 * c.z;
    let s_ = c.x - 0.08948418 * c.y - 1.2914855 * c.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return vec3<f32>(
        4.0767417 * l - 3.3077116 * m + 0.23096994 * s,
        -1.268438 * l + 2.6097574 * m - 0.34131938 * s,
        -0.0041960863 * l - 0.7034186 * m + 1.7076147 * s,
    );
}

fn chroma(c: vec3<f32>, p: Part) -> vec3<f32> {
    let lab = to_oklab(c);
    let amount = length(lab.yz);
    var factor = 1.0 + p.color.y;
    let vibrance = p.color.z;
    if vibrance != 0.0 {
        let muted = 1.0 - smoothstep(0.0, 0.25, amount);
        let hue = degrees(atan2(lab.z, lab.y));
        let skin = exp(-pow((hue - 60.0) / 25.0, 2.0));
        var protect = 1.0;
        if vibrance > 0.0 {
            protect = 1.0 - 0.7 * skin;
        }
        factor *= 1.0 + vibrance * muted * protect;
    }
    let ab = lab.yz * factor;
    let turned = vec2<f32>(ab.x * p.turn.x - ab.y * p.turn.y, ab.x * p.turn.y + ab.y * p.turn.x);
    return from_oklab(vec3<f32>(lab.x, turned));
}

fn hash(x: u32, y: u32) -> u32 {
    var h = (x * 0x8da6b343u) ^ (y * 0xd8163841u);
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    return h ^ (h >> 16u);
}

fn lattice(x: i32, y: i32) -> f32 {
    return f32(hash(bitcast<u32>(x), bitcast<u32>(y)) >> 8u) / 16777216.0 - 0.5;
}

fn value_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = p - cell;
    let s = f * f * (3.0 - 2.0 * f);
    let i = vec2<i32>(cell);
    let top = mix(lattice(i.x, i.y), lattice(i.x + 1, i.y), s.x);
    let bottom = mix(lattice(i.x, i.y + 1), lattice(i.x + 1, i.y + 1), s.x);
    return mix(top, bottom, s.y);
}

// Channel `c`'s curve on linear light.
fn curve(c: u32, value: f32) -> f32 {
    return to_linear(lookup(c, min(to_encoded(max(value, 0.0)), 1.0)));
}

// White balance, exposure, tone, curves (if `curves`) and chroma of one part, on linear light.
fn core(linear: vec3<f32>, p: Part, curves: bool) -> vec3<f32> {
    var lin = linear * p.gains.xyz;
    let y = max(luminance(lin), 0.0);
    let toned = to_linear(max(tone(to_encoded(y), p), 0.0));
    if y > 1e-6 {
        lin = lin * (toned / y);
    } else {
        lin = vec3<f32>(toned);
    }
    if curves {
        // Not a loop writing `lin[c]`: FXC must unroll a loop with such a write, and any loop around it (X3550, X3511).
        lin = vec3<f32>(curve(0u, lin.r), curve(1u, lin.g), curve(2u, lin.b));
    }
    if p.color.y != 0.0 || p.color.z != 0.0 || p.turn.y != 0.0 || p.turn.x != 1.0 {
        lin = chroma(lin, p);
    }
    return lin;
}

// `domain::edit::turn_hues`.
fn turn_hues(lin: vec3<f32>) -> vec3<f32> {
    let lab = to_oklab(lin);
    let at = fract(atan2(lab.z, lab.y) / 6.2831855) * 8.0;
    let i = u32(floor(at)) % 8u;
    let node = mix(edit.hue_nodes[i], edit.hue_nodes[(i + 1u) % 8u], fract(at));
    let colorful = min(length(lab.yz) / 0.1, 1.0);
    let ab = lab.yz * node.y;
    let cos_turn = cos(node.x);
    let sin_turn = sin(node.x);
    return from_oklab(vec3<f32>(
        lab.x + node.z * colorful,
        ab.x * cos_turn - ab.y * sin_turn,
        ab.x * sin_turn + ab.y * cos_turn,
    ));
}

fn part(lin: vec3<f32>, p: Part) -> vec3<f32> {
    if p.gains.w > 0.5 {
        return core(lin, p, false);
    }
    return lin;
}

// `domain::edit::MaskShape::weight`, `rgb` as stored.
fn shape_weight(i: u32, j: u32, rgb: vec3<f32>, uv: vec2<f32>, subject: f32) -> f32 {
    let kind = edit.masks[i].shapes[j].kind;
    let numbers = edit.masks[i].shapes[j].numbers;
    if kind.x > 4.5 {
        return textureSampleLevel(paintings, photo_sampler, uv, i32(kind.z + 0.5), 0.0).r;
    }
    if kind.x > 2.5 {
        // Without a subject, neither it nor the background selects anything.
        if (u32(edit.size.w) & 4u) == 0u {
            return 0.0;
        }
        if kind.x < 3.5 {
            return subject;
        }
        return 1.0 - subject;
    }
    if kind.x < 0.5 {
        let along = (numbers.xy - kind.zw) * edit.size.xy;
        let length_squared = dot(along, along);
        if length_squared > 0.0 {
            return 1.0 - smoothstep(0.0, 1.0, dot((uv - kind.zw) * edit.size.xy, along) / length_squared);
        }
        return 0.0;
    }
    let lin = vec3<f32>(to_linear(rgb.r), to_linear(rgb.g), to_linear(rgb.b));
    if kind.x < 1.5 {
        let value = to_encoded(max(luminance(lin), 0.0));
        let feather = max(numbers.x, 0.005);
        return smoothstep(kind.z - feather, kind.z, value) * (1.0 - smoothstep(kind.w, kind.w + feather, value));
    }
    let lab = to_oklab(lin);
    let turned = degrees(atan2(lab.z, lab.y)) - kind.z + 180.0;
    let away = abs(turned - 360.0 * floor(turned / 360.0) - 180.0);
    return (1.0 - smoothstep(kind.w, kind.w + max(numbers.x, 0.5), away)) * smoothstep(0.02, 0.06, length(lab.yz));
}

// `domain::edit::MaskParams::weight`: its shapes combined by `MaskOp::combine`.
fn mask_weight(i: u32, rgb: vec3<f32>, uv: vec2<f32>, subject: f32) -> f32 {
    var weight = shape_weight(i, 0u, rgb, uv, subject);
    for (var j = 1u; j < u32(edit.masks[i].info.y); j++) {
        let shape = shape_weight(i, j, rgb, uv, subject);
        let op = edit.masks[i].shapes[j].kind.y;
        if op < 0.5 {
            weight = max(weight, shape);
        } else if op < 1.5 {
            weight = min(weight, 1.0 - shape);
        } else {
            weight = min(weight, shape);
        }
    }
    if edit.masks[i].info.x > 0.5 {
        return 1.0 - weight;
    }
    return weight;
}

// `domain::edit::Params::halation_glow`.
fn halation_light(rgb: vec3<f32>) -> f32 {
    return smoothstep(0.3, 0.9, max(rgb.r, max(rgb.g, rgb.b)));
}

fn develop(rgb: vec3<f32>, uv: vec2<f32>, subject: f32, glow: vec3<f32>) -> vec3<f32> {
    let photo = Part(edit.gains, edit.tone, edit.color, edit.turn);
    var lin = core(vec3<f32>(to_linear(rgb.r), to_linear(rgb.g), to_linear(rgb.b)), photo, edit.gains.w > 0.5);
    if edit.film.z > 0.5 {
        lin = turn_hues(lin);
    }
    if (u32(edit.size.w) & 4u) != 0u {
        lin = mix(part(lin, edit.background), part(lin, edit.subject), subject);
    }
    for (var i = 0u; i < u32(edit.mask_count.x); i++) {
        lin = mix(lin, part(lin, edit.masks[i].part), mask_weight(i, rgb, uv, subject));
    }
    lin += glow;
    let vignette = edit.fx.x;
    if vignette != 0.0 {
        let centered = ((uv - edit.crop.xy) / edit.crop.zw - 0.5) * 2.0;
        let r = length(centered) / sqrt(2.0);
        let t = vignette * smoothstep(edit.fx.y, edit.fx.y + 0.5, r);
        if t < 0.0 {
            lin = lin * (1.0 + t);
        } else {
            lin = lin + t * max(1.0 - lin, vec3<f32>(0.0));
        }
    }
    lin = max(lin, vec3<f32>(0.0));
    var out = vec3<f32>(to_encoded(lin.r), to_encoded(lin.g), to_encoded(lin.b));
    let grain = edit.fx.z;
    if grain != 0.0 {
        let p = uv * edit.size.xy / edit.fx.w;
        let n = value_noise(p);
        // Each color channel's own noise, as a color film's three layers have.
        var own = vec3<f32>(n);
        if edit.film.y > 0.0 {
            own = vec3<f32>(value_noise(p + vec2<f32>(37.0, 17.0)), n, value_noise(p + vec2<f32>(59.0, 91.0)));
        }
        var amount = grain;
        if edit.film.x > 0.5 {
            let v = clamp((out.r + out.g + out.b) / 3.0, 0.0, 1.0);
            amount = grain * (0.35 + 0.65 * 4.0 * v * (1.0 - v));
        }
        out += (n + (own - n) * edit.film.y) * amount * 0.2;
    }
    return clamp(out, vec3<f32>(0.0), vec3<f32>(1.0));
}

// `domain::edit::Params::blur_sigma`: the blur at `uv` as σ in pixels of the full photo.
fn blur_sigma(uv: vec2<f32>, subject: f32) -> f32 {
    let kind = u32(edit.blur.y + 0.5);
    // A background blur waits for the subject.
    if edit.blur.x <= 0.0 || kind == 0u || (kind == 1u && (u32(edit.size.w) & 4u) == 0u) {
        return 0.0;
    }
    let size = edit.size.xy;
    let longer = max(size.x, size.y);
    let offset = (uv - edit.focus.xy) * size;
    var weight = 1.0 - clamp(subject, 0.0, 1.0);
    if kind == 2u {
        weight = smoothstep(edit.blur.z, edit.blur.z + edit.blur.w, abs(offset.y) / longer);
    } else if kind == 3u {
        weight = smoothstep(edit.blur.z, edit.blur.z + edit.blur.w, length(offset) / longer);
    }
    return weight * edit.blur.x * longer;
}

// Mip level `level` at `uv` through a cubic B-spline, from four bilinear samples.
fn bspline(level: u32, uv: vec2<f32>) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(photo, level));
    let p = uv * dims - 0.5;
    let i = floor(p);
    let f = p - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - f) * (1.0 - f) * (1.0 - f) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 0.5 + w1 / g0) / dims;
    let h1 = (i + 1.5 + w3 / g1) / dims;
    let l = f32(level);
    let a = textureSampleLevel(photo, photo_sampler, vec2<f32>(h0.x, h0.y), l);
    let b = textureSampleLevel(photo, photo_sampler, vec2<f32>(h1.x, h0.y), l);
    let c = textureSampleLevel(photo, photo_sampler, vec2<f32>(h0.x, h1.y), l);
    let d = textureSampleLevel(photo, photo_sampler, vec2<f32>(h1.x, h1.y), l);
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

// `domain::edit::Pyramid::blurred`: `sharp` at lod 0, else between the levels around `lod`.
fn blurred(sharp: vec4<f32>, uv: vec2<f32>, sigma: f32) -> vec4<f32> {
    let levels = textureNumLevels(photo);
    let base = f32(textureDimensions(photo, 0u).x) / edit.size.x;
    let lod = clamp(log2(sigma * base / 0.65), 0.0, f32(levels - 1u));
    if lod <= 0.0 {
        return sharp;
    }
    let low = u32(floor(lod));
    let t = lod - f32(low);
    var below = sharp;
    if low > 0u {
        below = bspline(low, uv);
    }
    if t <= 0.0 {
        return below;
    }
    return mix(below, bspline(low + 1u, uv), t);
}

// `domain::edit::Geometry::to_original`: where a point of the output lies on the photo.
fn to_original(p: vec2<f32>) -> vec2<f32> {
    var q = p;
    switch u32(edit.focus.z) {
        case 1u: { q = vec2<f32>(p.y, 1.0 - p.x); }
        case 2u: { q = vec2<f32>(1.0 - p.x, 1.0 - p.y); }
        case 3u: { q = vec2<f32>(1.0 - p.y, p.x); }
        default: {}
    }
    return edit.crop.xy + q * edit.crop.zw;
}

@fragment
fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let t = (frag.xy - edit.rect.xy) / (edit.rect.zw - edit.rect.xy);
    let uv = to_original(mix(edit.uv.xy, edit.uv.zw, t));
    let texel = textureSample(photo, photo_sampler, uv);
    let subject = textureSample(subject_mask, photo_sampler, uv).r;
    // Outside the photo: the triangle covers the whole viewport.
    if any(t < vec2<f32>(0.0)) || any(t > vec2<f32>(1.0)) {
        discard;
    }
    let flags = u32(edit.size.w);
    var rgb = texel.rgb;
    var stored = texel.rgb;
    if (flags & 1u) == 0u {
        let sigma = blur_sigma(uv, subject);
        if sigma > 0.0 {
            rgb = blurred(texel, uv, sigma).rgb;
            stored = rgb;
        }
        var glow = vec3<f32>(0.0);
        if edit.film.w > 0.0 {
            // `domain::edit::Params::halation_sigma`.
            let around = blurred(texel, uv, 0.006 * max(edit.size.x, edit.size.y));
            let light = max(halation_light(around.rgb) - halation_light(texel.rgb), 0.0);
            glow = vec3<f32>(1.0, 0.22, 0.06) * light * edit.film.w;
        }
        rgb = develop(rgb, uv, subject, glow);
    }
    if (flags & 8u) != 0u {
        var selected = subject;
        if (flags & 16u) != 0u {
            selected = 1.0 - subject;
        }
        rgb = mix(rgb, vec3<f32>(1.0, 0.25, 0.25), 0.45 * selected);
    }
    // The chosen mask (index + 1) tinted (REQ-030-07).
    if (flags >> 8u) != 0u {
        rgb = mix(rgb, vec3<f32>(1.0, 0.25, 0.25), 0.45 * mask_weight((flags >> 8u) - 1u, stored, uv, subject));
    }
    if (flags & 2u) != 0u {
        rgb = vec3<f32>(to_linear(rgb.r), to_linear(rgb.g), to_linear(rgb.b));
    }
    let alpha = texel.a * edit.size.z;
    // Premultiplied, as egui blends.
    return vec4<f32>(rgb * alpha, alpha);
}
