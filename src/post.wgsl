// Display effects (Rebuild Prompt, "Palette-Specific Materials And Display
// Effects"): a second pass over the finished scene, applying the palette's
// monitor effect, then the message box, which stays clear of every effect.
//
// Effects by palette: Amber, Apple II, C64, and both CGAs get monitor
// scanlines (and their own color treatment); Game Boy gets an LCD grid
// (always, not controlled by the Display Effects switch). Every other
// palette passes through unchanged. Monitor effects work in encoded sRGB on
// device pixels, screen-fixed: they never move with the scene.

struct PostParams {
    // x: effect (0 none, 1 Amber, 2 Apple II, 3 C64, 4 CGA, 5 Game Boy);
    // y: neighbor reach in device pixels; z: device pixels per cell;
    // w: scanline period in device pixels.
    effect: vec4<f32>,
    // x: dark scanline rows per period; y: Game Boy gap width in device
    // pixels; z, w unused.
    lines: vec4<f32>,
    // The message box: x, y, width, height in device pixels (width 0 hides).
    message: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var message_box: texture_2d<f32>;
@group(0) @binding(2) var<uniform> post: PostParams;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((vertex_index << 1u) & 2u);
    let y = f32(vertex_index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    let c = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn decode_srgb(encoded: vec3<f32>) -> vec3<f32> {
    let c = clamp(encoded, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// The scene's encoded sRGB color at device pixel `p` (edges clamped).
fn source(p: vec2<i32>) -> vec3<f32> {
    let dims = vec2<i32>(textureDimensions(scene));
    let q = clamp(p, vec2<i32>(0), dims - vec2<i32>(1));
    return encode_srgb(textureLoad(scene, q, 0).rgb);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

fn chroma(c: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(dot(c, vec3<f32>(0.596, -0.274, -0.322)), dot(c, vec3<f32>(0.211, -0.523, 0.312)));
}

fn yiq_to_rgb(y: f32, iq: vec2<f32>) -> vec3<f32> {
    return clamp(
        vec3<f32>(
            y + 0.956 * iq.x + 0.621 * iq.y,
            y - 0.272 * iq.x - 0.647 * iq.y,
            y - 1.106 * iq.x + 1.703 * iq.y,
        ),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}

// True on the dark rows of the screen-fixed scanline pattern.
fn dark_row(y: i32) -> bool {
    let period = max(i32(post.effect.w), 2);
    return y % period >= period - i32(post.lines.x);
}

// The tight highlight halo (C64, CGA): four axial neighbors at `reach`,
// each decoded to linear and weighted by how bright it is, screen-blended
// over the linear color `base`; returns encoded sRGB.
fn halo(p: vec2<i32>, reach: i32, base: vec3<f32>, weight: f32) -> vec3<f32> {
    var glow = vec3<f32>(0.0);
    let offsets = array<vec2<i32>, 4>(vec2<i32>(reach, 0), vec2<i32>(-reach, 0), vec2<i32>(0, reach), vec2<i32>(0, -reach));
    for (var i = 0; i < 4; i++) {
        let neighbor = source(p + offsets[i]);
        let strength = smoothstep(0.55, 0.85, max(max(neighbor.r, neighbor.g), neighbor.b)) * weight;
        glow += decode_srgb(neighbor) * strength;
    }
    let screened = vec3<f32>(1.0) - (vec3<f32>(1.0) - base) * (vec3<f32>(1.0) - clamp(glow, vec3<f32>(0.0), vec3<f32>(1.0)));
    return encode_srgb(screened);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(position.xy));
    let effect = u32(post.effect.x);
    let reach = max(i32(post.effect.y), 1);
    var color = source(p);
    if (effect == 1u) {
        // Amber: whole-scene scanlines, dark rows keeping 75%.
        if (dark_row(p.y)) {
            color *= 0.75;
        }
    } else if (effect == 2u) {
        // Apple II: composite chroma bleed and alternating-column fringes.
        let r = vec2<i32>(reach, 0);
        let c0 = color;
        let l1 = source(p - r);
        let r1 = source(p + r);
        let l2 = source(p - 2 * r);
        let r2 = source(p + 2 * r);
        let y = 0.84 * luma(c0) + 0.08 * (luma(l1) + luma(r1));
        var iq = 0.4 * chroma(c0) + 0.24 * (chroma(l1) + chroma(r1)) + 0.06 * (chroma(l2) + chroma(r2));
        let phase = select(-1.0, 1.0, (p.x / reach) % 2 == 0);
        iq += vec2<f32>(0.12, 0.08) * (luma(r1) - luma(l1)) * phase;
        color = yiq_to_rgb(y, iq);
        if (dark_row(p.y)) {
            color *= 0.7;
        }
    } else if (effect == 3u) {
        // C64: milder chroma bleed (luma unchanged) and a tight halo.
        let r = vec2<i32>(reach, 0);
        let l1 = source(p - r);
        let r1 = source(p + r);
        let iq = 0.7 * chroma(color) + 0.15 * (chroma(l1) + chroma(r1));
        let reconstructed = yiq_to_rgb(luma(color), iq);
        color = halo(p, reach, decode_srgb(reconstructed), 0.045);
        if (dark_row(p.y)) {
            color *= 0.8;
        }
    } else if (effect == 4u) {
        // CGA: a crisp RGB monitor: faint halo, then reach-wide R/G/B
        // stripes (the matching channel keeps 100%, the others 96%).
        color = halo(p, reach, decode_srgb(color), 0.012);
        let stripe = (p.x / reach) % 3;
        var mask = vec3<f32>(0.96);
        mask[stripe] = 1.0;
        color *= mask;
        if (dark_row(p.y)) {
            color *= 0.75;
        }
    } else if (effect == 5u) {
        // Game Boy: dark gaps at the end of each cell, across and down,
        // rgba(3, 19, 3, 0.55) each; where they cross they compound.
        let cell = max(i32(post.effect.z), 1);
        let gap = max(i32(post.lines.y), 1);
        let gap_color = vec3<f32>(3.0, 19.0, 3.0) / 255.0;
        if (p.x % cell >= cell - gap) {
            color = mix(color, gap_color, 0.55);
        }
        if (p.y % cell >= cell - gap) {
            color = mix(color, gap_color, 0.55);
        }
    }
    // The message box, premultiplied sRGB, over the effect (it stays clear
    // of every effect).
    let local = vec2<i32>(floor(position.xy - post.message.xy));
    let size = vec2<i32>(post.message.zw);
    if (post.message.z > 0.0 && all(local >= vec2<i32>(0, 0)) && all(local < size)) {
        let box_color = textureLoad(message_box, local, 0);
        color = box_color.rgb + color * (1.0 - box_color.a);
    }
    return vec4<f32>(decode_srgb(color), 1.0);
}
