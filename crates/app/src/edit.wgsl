// The edit preview (spec 021): the unedited original drawn through the recipe. The formulas mirror
// `domain::edit::Params::pixel`; a test compares both.

struct Edit {
    // The photo's rect in framebuffer pixels: min (xy), max (zw).
    rect: vec4<f32>,
    // Texture coordinates at the rect's min (xy) and max (zw).
    uv: vec4<f32>,
    // The photo's full size in pixels (xy), opacity (z), flags (w): 1 = the original (compare), 2 = linear output.
    size: vec4<f32>,
    // White balance gains times exposure (xyz), curves on (w).
    gains: vec4<f32>,
    // Black point, white point, shadows, highlights.
    tone: vec4<f32>,
    // Contrast, saturation, vibrance, brightness (the midtones' gamma).
    color: vec4<f32>,
    // Vignette, vignette start, grain, grain cell size.
    fx: vec4<f32>,
    curves: array<vec4<f32>, 256>,
}

@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var photo_sampler: sampler;
@group(0) @binding(2) var<uniform> edit: Edit;

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

fn tone(value: f32) -> f32 {
    var l = (value - edit.tone.x) / (edit.tone.y - edit.tone.x);
    if edit.color.w != 1.0 && l > 0.0 && l < 1.0 {
        l = pow(l, edit.color.w);
    }
    let t = clamp(l, 0.0, 1.0);
    l += edit.tone.z * 6.75 * t * (1.0 - t) * (1.0 - t);
    l += edit.tone.w * 6.75 * (1.0 - t) * t * t;
    let contrast = edit.color.x;
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

fn chroma(c: vec3<f32>) -> vec3<f32> {
    let lab = to_oklab(c);
    let amount = length(lab.yz);
    var factor = 1.0 + edit.color.y;
    let vibrance = edit.color.z;
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
    return from_oklab(vec3<f32>(lab.x, lab.yz * factor));
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

fn develop(rgb: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    var lin = vec3<f32>(to_linear(rgb.r), to_linear(rgb.g), to_linear(rgb.b)) * edit.gains.xyz;
    let y = max(luminance(lin), 0.0);
    let toned = to_linear(max(tone(to_encoded(y)), 0.0));
    if y > 1e-6 {
        lin = lin * (toned / y);
    } else {
        lin = vec3<f32>(toned);
    }
    if edit.gains.w > 0.5 {
        for (var c = 0u; c < 3u; c++) {
            lin[c] = to_linear(lookup(c, min(to_encoded(max(lin[c], 0.0)), 1.0)));
        }
    }
    if edit.color.y != 0.0 || edit.color.z != 0.0 {
        lin = chroma(lin);
    }
    let vignette = edit.fx.x;
    if vignette != 0.0 {
        let centered = (uv - 0.5) * 2.0;
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
        out += value_noise(uv * edit.size.xy / edit.fx.w) * grain * 0.2;
    }
    return clamp(out, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let t = (frag.xy - edit.rect.xy) / (edit.rect.zw - edit.rect.xy);
    let uv = mix(edit.uv.xy, edit.uv.zw, t);
    let texel = textureSample(photo, photo_sampler, uv);
    // Outside the photo: the triangle covers the whole viewport.
    if any(t < vec2<f32>(0.0)) || any(t > vec2<f32>(1.0)) {
        discard;
    }
    let flags = u32(edit.size.w);
    var rgb = texel.rgb;
    if (flags & 1u) == 0u {
        rgb = develop(rgb, uv);
    }
    if (flags & 2u) != 0u {
        rgb = vec3<f32>(to_linear(rgb.r), to_linear(rgb.g), to_linear(rgb.b));
    }
    let alpha = texel.a * edit.size.z;
    // Premultiplied, as egui blends.
    return vec4<f32>(rgb * alpha, alpha);
}
