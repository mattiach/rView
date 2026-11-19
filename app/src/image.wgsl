// Image quad: sample linear premultiplied colour through an sRGB view,
// composite over the background in linear light, encode sRGB in the shader (surface is not sRGB).

struct U {
    m0: vec4f,   // ndc.x = m0.x*x + m0.y*y + m0.z   (x, y in image pixels)
    m1: vec4f,   // ndc.y = m1.x*x + m1.y*y + m1.z
    bg: vec4f,   // background, linear
}

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(1) @binding(0) var tex: texture_2d<f32>;

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs(@location(0) p: vec2f, @location(1) uv: vec2f) -> VOut {
    var o: VOut;
    let x = u.m0.x * p.x + u.m0.y * p.y + u.m0.z;
    let y = u.m1.x * p.x + u.m1.y * p.y + u.m1.z;
    o.pos = vec4f(x, y, 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn encode_srgb(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs(i: VOut) -> @location(0) vec4f {
    let s = textureSample(tex, samp, i.uv);
    let c = s.rgb + u.bg.rgb * (1.0 - s.a);
    return vec4f(encode_srgb(c), 1.0);
}

// Mip generation: full-screen triangle sampling the previous level; the target is an sRGB view,
// so averaging happens in linear light and the result is re-encoded by the hardware.
struct MOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn mip_vs(@builtin(vertex_index) i: u32) -> MOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: MOut;
    o.pos = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@group(0) @binding(0) var mip_src: texture_2d<f32>;
@group(0) @binding(1) var mip_samp: sampler;

@fragment
fn mip_fs(i: MOut) -> @location(0) vec4f {
    return textureSample(mip_src, mip_samp, i.uv);
}
