// Display effects (Rebuild Prompt, "Palette-Specific Materials And Display
// Effects", plus owner additions of 2026-11-05): a second pass over the
// finished scene, applying the palette's monitor effect, then the message
// box, which stays clear of every effect (only Amber and Green give its
// text a phosphor bloom).
//
// Effects by palette (`effect` below):
//   1 Amber      scanlines, phosphor persistence, vignette, text bloom
//   2 Apple II   composite color bleed and fringes, scanlines, convergence
//                error, vignette
//   3 C64        color bleed and highlight halo, scanlines, convergence
//                error, vignette
//   4 CGA        RGB stripes and halo, scanlines, convergence error,
//                vignette
//   5 Game Boy   LCD grid and ghosting (always, not the Display Effects
//                switch)
//   6 Green      phosphor persistence, vignette, text bloom
//   7 EGA        scanlines, vignette (owner: "get those on the EGA and VGA
//   8 VGA        palette too"; the spec leaves them unprocessed)
//   0 none       Classic, ArtDeco, Parchment, or effects switched off
//
// Monitor effects work in encoded sRGB on device pixels and are
// screen-fixed: they never move with the scene. The CRT power-on (a line
// opening out to the picture) plays once after switching into a CRT palette.
// The pass also writes this frame (before the message box) to a history
// texture, which the next frame reads for ghosting and persistence.

struct PostParams {
    // x: effect (see above); y: neighbor reach in device pixels; z: device
    // pixels per cell; w: scanline period in device pixels.
    effect: vec4<f32>,
    // x: dark scanline rows per period; y: Game Boy gap width in device
    // pixels; z: the screen's width and w its height in device pixels.
    lines: vec4<f32>,
    // The message box: x, y, width, height in device pixels (width 0 hides).
    message: vec4<f32>,
    // x: how much of the previous frame a Game Boy pixel keeps (LCD
    // ghosting); y: how much of it a phosphor keeps (persistence); z: the
    // CRT power-on's progress (1 when done); w: where the Amber and Green
    // refresh beam is, 0 at the top of the screen to 1 at the bottom.
    history: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var message_box: texture_2d<f32>;
@group(0) @binding(2) var<uniform> post: PostParams;
@group(0) @binding(3) var previous: texture_2d<f32>;

// Color bleed is stronger than the Rebuild Prompt's (owner, 2026-11-05:
// "color bleed effects in some of palettes could be a little stronger"):
// chroma is sampled this many times the neighbor reach away, and weighted
// further from the center. Luma keeps the spec's weights, so edges stay
// readable while color smears.
const BLEED_SPREAD: i32 = 2;
// Convergence error: red and blue drift this many reaches apart at the
// screen's left and right edges, nothing at the center.
const CONVERGENCE: f32 = 1.5;
// Vignette: how much the corners darken.
const VIGNETTE: f32 = 0.22;
// Scanline dark rows keep this much on EGA and VGA.
const EGA_ROWS: f32 = 0.75;
const VGA_ROWS: f32 = 0.82;
// The Amber and Green refresh beam (owner, 2026-11-05): how much brighter
// the beam's line is, how much the freshly drawn phosphor behind it starts
// brighter, how far (a share of the screen's height) that fades over, and
// how thick the line is in scanline periods.
const BEAM_LINE: f32 = 0.3;
const BEAM_TRAIL: f32 = 0.18;
const BEAM_FADE: f32 = 0.08;
const BEAM_THICKNESS: f32 = 1.0;
// Phosphor bloom around Amber and Green message text.
const TEXT_BLOOM: f32 = 0.9;

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

// One scene texel's encoded sRGB color (edges clamped).
fn texel(p: vec2<i32>) -> vec3<f32> {
    let dims = vec2<i32>(textureDimensions(scene));
    let q = clamp(p, vec2<i32>(0), dims - vec2<i32>(1));
    return encode_srgb(textureLoad(scene, q, 0).rgb);
}

// The scene at a fractional horizontal position (linear between texels).
fn texel_at(x: f32, y: i32) -> vec3<f32> {
    let left = floor(x);
    let t = x - left;
    return mix(texel(vec2<i32>(i32(left), y)), texel(vec2<i32>(i32(left) + 1, y)), t);
}

fn has_convergence_error() -> bool {
    let effect = u32(post.effect.x);
    return effect == 2u || effect == 3u || effect == 4u;
}

// The scene's encoded sRGB color at device pixel `p`, with the CRT
// palettes' convergence error: red drifts outward and blue inward toward
// the screen's sides (sub-pixel, so it grows smoothly).
fn source(p: vec2<i32>) -> vec3<f32> {
    if (!has_convergence_error()) {
        return texel(p);
    }
    let half = max(post.lines.z * 0.5, 1.0);
    let off = CONVERGENCE * post.effect.y * (f32(p.x) - half) / half;
    let x = f32(p.x);
    return vec3<f32>(texel_at(x - off, p.y).r, texel(p).g, texel_at(x + off, p.y).b);
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

// The message box's premultiplied sRGB texel at `local` (transparent
// outside).
fn box_texel(local: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(post.message.zw);
    if (any(local < vec2<i32>(0, 0)) || any(local >= size)) {
        return vec4<f32>(0.0);
    }
    return textureLoad(message_box, local, 0);
}

struct Output {
    // What the screen shows.
    @location(0) screen: vec4<f32>,
    // This frame before the message box, for the next frame's ghosting
    // and persistence.
    @location(1) history: vec4<f32>,
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> Output {
    let p = vec2<i32>(floor(position.xy));
    let effect = u32(post.effect.x);
    let reach = max(i32(post.effect.y), 1);
    var color = source(p);
    // Ghosting (Game Boy's slow LCD) and phosphor persistence (Amber,
    // Green), from the previous frame.
    let before = encode_srgb(textureLoad(previous, p, 0).rgb);
    if (effect == 5u) {
        color = mix(color, before, post.history.x);
    } else if (effect == 1u || effect == 6u) {
        color = max(color, before * post.history.y);
    }
    let remembered = color;
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
        let y = 0.84 * luma(c0) + 0.08 * (luma(l1) + luma(r1));
        // Chroma (stronger than the spec's 0.4/0.24/0.06 at 1x/2x reach).
        let b = BLEED_SPREAD * r;
        let cl1 = source(p - b);
        let cr1 = source(p + b);
        let cl2 = source(p - 2 * b);
        let cr2 = source(p + 2 * b);
        var iq = 0.3 * chroma(c0) + 0.22 * (chroma(cl1) + chroma(cr1)) + 0.13 * (chroma(cl2) + chroma(cr2));
        let phase = select(-1.0, 1.0, (p.x / reach) % 2 == 0);
        iq += vec2<f32>(0.12, 0.08) * (luma(r1) - luma(l1)) * phase;
        color = yiq_to_rgb(y, iq);
        if (dark_row(p.y)) {
            color *= 0.7;
        }
    } else if (effect == 3u) {
        // C64: color bleed (luma unchanged) and a tight halo.
        // Chroma (stronger than the spec's 0.7/0.15 at 1x reach).
        let b = vec2<i32>(BLEED_SPREAD * reach, 0);
        let l1 = source(p - b);
        let r1 = source(p + b);
        let iq = 0.5 * chroma(color) + 0.25 * (chroma(l1) + chroma(r1));
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
    } else if (effect == 7u || effect == 8u) {
        // EGA and VGA: scanlines only (VGA monitors were finer, so a
        // lighter dark row).
        if (dark_row(p.y)) {
            color *= select(EGA_ROWS, VGA_ROWS, effect == 8u);
        }
    }
    // Amber and Green: the refresh beam sweeping down, the phosphor it has
    // just drawn a little brighter and fading behind it.
    if (effect == 1u || effect == 6u) {
        let height = max(post.lines.w, 1.0);
        let beam = post.history.w * height;
        // Rows since the beam passed (wrapping from the bottom to the top).
        let behind = (beam - position.y + height) % height;
        let period = max(post.effect.w, 2.0);
        let line = 1.0 - smoothstep(0.0, BEAM_THICKNESS * period, behind);
        let trail = exp(-behind / (BEAM_FADE * height));
        color = min(color * (1.0 + BEAM_LINE * line + BEAM_TRAIL * trail), vec3<f32>(1.0));
    }
    // Every CRT palette: a gentle vignette toward the corners.
    let crt = effect != 0u && effect != 5u;
    let screen = max(post.lines.zw, vec2<f32>(1.0));
    if (crt) {
        let from_center = (position.xy / screen - vec2<f32>(0.5)) * vec2<f32>(2.0, 2.0);
        let corner = clamp(dot(from_center, from_center) * 0.5, 0.0, 1.0);
        color *= 1.0 - VIGNETTE * corner * corner;
    }
    // The message box, premultiplied sRGB, over the effect. Amber and
    // Green text glows a little, like a phosphor terminal.
    let local = vec2<i32>(floor(position.xy - post.message.xy));
    if (post.message.z > 0.0) {
        if (effect == 1u || effect == 6u) {
            var glow = vec3<f32>(0.0);
            let reach_box = 2 * reach;
            let offsets = array<vec2<i32>, 8>(
                vec2<i32>(reach_box, 0), vec2<i32>(-reach_box, 0), vec2<i32>(0, reach_box), vec2<i32>(0, -reach_box),
                vec2<i32>(2 * reach_box, 0), vec2<i32>(-2 * reach_box, 0), vec2<i32>(0, 2 * reach_box), vec2<i32>(0, -2 * reach_box),
            );
            for (var i = 0; i < 8; i++) {
                let weight = select(0.5, 1.0, i < 4) / 6.0;
                let t = box_texel(local + offsets[i]);
                // The text (bright) glows; the box's dim fill does not.
                glow += t.rgb * smoothstep(0.35, 0.7, max(max(t.r, t.g), t.b)) * weight;
            }
            color = color + glow * TEXT_BLOOM;
        }
        let box_color = box_texel(local);
        color = box_color.rgb + color * (1.0 - box_color.a);
    }
    // The CRT power-on: the picture opens out of a bright line through the
    // middle, then settles.
    let progress = clamp(post.history.z, 0.0, 1.0);
    if (crt && progress < 1.0) {
        let open = smoothstep(0.0, 0.7, progress);
        let half_height = max(open * screen.y * 0.5, 1.0);
        let from_middle = abs(position.y - screen.y * 0.5);
        if (from_middle > half_height) {
            color = vec3<f32>(0.0);
        } else {
            color = min(color * (1.0 + 1.5 * (1.0 - progress)), vec3<f32>(1.0));
        }
    }
    var out: Output;
    out.screen = vec4<f32>(decode_srgb(color), 1.0);
    out.history = vec4<f32>(decode_srgb(remembered), 1.0);
    return out;
}
