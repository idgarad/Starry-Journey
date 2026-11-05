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
//   6 Green      falling code rain, phosphor persistence, vignette, text
//                bloom
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
    // x: the Green code rain's clock in seconds; y: device pixels per CSS
    // pixel (the display scale).
    rain: vec4<f32>,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var message_box: texture_2d<f32>;
@group(0) @binding(2) var<uniform> post: PostParams;
@group(0) @binding(3) var previous: texture_2d<f32>;
// The Green code rain's 128x160 glyph atlas (src/rain.rs): 16 columns of
// 8x16 slots, 157 GNU Unifont glyphs.
@group(0) @binding(4) var glyphs: texture_2d<f32>;

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
const BEAM_LINE: f32 = 0.12;
const BEAM_TRAIL: f32 = 0.3;
const BEAM_FADE: f32 = 0.15;
const BEAM_THICKNESS: f32 = 1.0;
// Phosphor bloom on Amber and Green: each of four neighbors' share of glow
// at rest, and the extra where the beam has just passed.
const PHOSPHOR_BLOOM: f32 = 0.05;
const BEAM_BLOOM: f32 = 0.12;
// Phosphor bloom around Amber and Green message text.
const TEXT_BLOOM: f32 = 0.9;
// The Green code rain (Rebuild Prompt, "Green"): cells in CSS pixels, the
// glyph's offset in its cell, and the share of columns that rain.
const RAIN_CELL: vec2<f32> = vec2<f32>(18.0, 20.0);
// Each glyph pixel is this many device pixels wide (times the display
// scale): owner, 2026-11-05, "the glyphs need to be a bit bigger ... 3
// texel wide". The spec's cells, offsets, and halo scale with it.
const RAIN_PIXEL: f32 = 3.0;
const RAIN_GLYPH_OFFSET: vec2<i32> = vec2<i32>(5, 2);
const RAIN_COLUMNS: f32 = 0.42;
const RAIN_GLYPHS: f32 = 157.0;
// Tails last this many rows; the wrap adds this many rows below the screen.
const RAIN_TAIL: f32 = 9.0;
const RAIN_WRAP: f32 = 12.0;
// Tails are the palette's green (#33ff66) and heads pale green, linear.
const RAIN_TAIL_COLOR: vec3<f32> = vec3<f32>(0.0331, 1.0, 0.1329);
const RAIN_HEAD_COLOR: vec3<f32> = vec3<f32>(0.4, 1.0, 0.45);
// The screen blend's heaviest weight, and how much the scene darkens
// under a glyph and in its one-pixel outline.
const RAIN_MAX_WEIGHT: f32 = 0.7;
const RAIN_DARKEN: f32 = 0.6;
// Stronger than the spec's (owner, 2026-11-05: "strengthen the scanlines
// and dial up the phosphor glow"): alternate glyph rows keep this much
// (spec 0.75), and the halo's near and far neighbor weights and cap (spec
// 0.045, 0.015, 0.12).
const RAIN_ROW_KEEP: f32 = 0.5;
const RAIN_HALO_NEAR: f32 = 0.11;
const RAIN_HALO_FAR: f32 = 0.05;
const RAIN_HALO_CAP: f32 = 0.35;
// When the refresh beam passes a glyph it flares (owner, 2026-11-05): its
// emission grows by up to this much, fading over this share of the screen
// height behind the beam, and the blend may then reach full weight.
const RAIN_BURST: f32 = 1.5;
const RAIN_BURST_FADE: f32 = 0.04;

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

// The spec's hash: `fract(sin(value*12.9898)*43758.5453)`.
fn rain_hash(value: f32) -> f32 {
    return fract(sin(value * 12.9898) * 43758.5453);
}

// Glyph `index`'s bit (0 or 1) at `local` (pixels in its 8x16 slot; 0
// outside it).
fn glyph_bit(index: u32, local: vec2<i32>) -> f32 {
    if (any(local < vec2<i32>(0)) || local.x >= 8 || local.y >= 16) {
        return 0.0;
    }
    let slot = vec2<i32>(i32(index % 16u) * 8, i32(index / 16u) * 16);
    return textureLoad(glyphs, slot + local, 0).r;
}

// How strongly rain cell `cell` (column, row) is lit, 0 when it is not:
// 1 at a column's head, its tail fading `0.5*(1-d/9)^2` over the nine rows
// above. Heads fall 3 to 6 rows a second by column, wrapping over the
// visible rows plus 12.
fn rain_strength(cell: vec2<f32>, rows: f32) -> f32 {
    let column = cell.x + 1.0;
    if (rain_hash(column) >= RAIN_COLUMNS) {
        return 0.0;
    }
    let phase = rain_hash(column + 0.371);
    let speed = 3.0 + 3.0 * phase;
    let span = rows + RAIN_WRAP;
    let head = floor((post.rain.x * speed + rain_hash(column + 0.713) * span) % span);
    let d = head - cell.y;
    if (d < 0.0 || d >= RAIN_TAIL) {
        return 0.0;
    }
    if (d == 0.0) {
        return 1.0;
    }
    let fade = 1.0 - d / RAIN_TAIL;
    return 0.5 * fade * fade;
}

// The Green code rain over the encoded sRGB `color` at device position
// `position`; returns encoded sRGB. Only pixels in a lit cell change: the
// scene darkens under the glyph and in a one-pixel outline round it, then
// the glyph (alternate rows 25% darker) and a small halo are screen-blended
// on in linear light.
fn code_rain(color: vec3<f32>, position: vec2<f32>) -> vec3<f32> {
    let scale = max(post.rain.y, 0.5) * RAIN_PIXEL;
    let css = position / scale;
    let cell = floor(css / RAIN_CELL);
    let rows = ceil(max(post.lines.w, 1.0) / scale / RAIN_CELL.y);
    let strength = rain_strength(cell, rows);
    if (strength <= 0.0) {
        return color;
    }
    let pick = rain_hash(cell.x * 31.7 + cell.y * 1.913 + 0.37);
    let index = u32(min(floor(pick * RAIN_GLYPHS), RAIN_GLYPHS - 1.0));
    let local = vec2<i32>(floor(css - cell * RAIN_CELL)) - RAIN_GLYPH_OFFSET;
    let bit = glyph_bit(index, local);
    // The four axial neighbors one and two pixels away (written out: a
    // loop over a vector array fails to compile on DX12, error X3500).
    let dx = vec2<i32>(1, 0);
    let dy = vec2<i32>(0, 1);
    let near = glyph_bit(index, local + dx) + glyph_bit(index, local - dx) + glyph_bit(index, local + dy) + glyph_bit(index, local - dy);
    let far = glyph_bit(index, local + 2 * dx) + glyph_bit(index, local - 2 * dx) + glyph_bit(index, local + 2 * dy) + glyph_bit(index, local - 2 * dy);
    let outline = max(bit, min(near, 1.0));
    var linear = decode_srgb(color);
    // Darkening scales with the scene's own brightness (a multiply), so
    // dark sky barely changes and lit windows behind a glyph give way.
    linear *= 1.0 - RAIN_DARKEN * outline * min(strength * 2.0, 1.0);
    let row_shade = select(1.0, RAIN_ROW_KEEP, (local.y & 1) == 1);
    let halo_glow = min(RAIN_HALO_NEAR * near + RAIN_HALO_FAR * far, RAIN_HALO_CAP) * (1.0 - bit);
    // The refresh beam's flare: strongest just behind the beam, fading
    // within a few glyph rows (once per sweep, so no flashing).
    let height = max(post.lines.w, 1.0);
    let behind = (post.history.w * height - position.y + height) % height;
    let burst = exp(-behind / (RAIN_BURST_FADE * height));
    let emission = (bit * row_shade + halo_glow) * (1.0 + RAIN_BURST * burst);
    let weight = min(strength * emission, 1.0) * mix(RAIN_MAX_WEIGHT, 1.0, burst);
    let head = select(0.0, 1.0, strength >= 1.0);
    let tint = mix(RAIN_TAIL_COLOR, RAIN_HEAD_COLOR, max(head, 0.6 * burst));
    linear = vec3<f32>(1.0) - (vec3<f32>(1.0) - linear) * (vec3<f32>(1.0) - tint * weight);
    return encode_srgb(linear);
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
    // Green: the falling code rain, before persistence so the phosphor
    // keeps a trace of each glyph as the heads move on.
    if (effect == 6u) {
        color = code_rain(color, position.xy);
    }
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
        // Phosphor bloom (owner, 2026-11-05, from a real amber terminal):
        // lit pixels glow softly into the dark around them, one and two
        // reaches out, more where the beam has just refreshed them.
        let energy = max(line, trail);
        let weight = PHOSPHOR_BLOOM + BEAM_BLOOM * energy;
        color = halo(p, reach, decode_srgb(color), weight);
        color = halo(p, 2 * reach, decode_srgb(color), weight * 0.5);
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
