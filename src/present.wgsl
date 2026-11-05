// Presents the scene. Every texture holds one texel per logical cell (the
// message box excepted); each cell covers a cell x cell block of device
// pixels from the top-left.
//
// Draw order (Rebuild Prompt, "Final draw order"), in linear light:
// 1. background: the sky and stars;
// 2. the moon (or battle station), at its exact fractional position;
// 2b. the procedural clouds (src/skyclouds.rs) in four layers far to
//    near, lit on a background thread, each slid with its own wind and
//    share of the Endless camera, in front of the moon and behind the city;
//    the city's glow (src/cityglow.rs) lights them here, every frame, so
//    it stays under the windows that cast it. Before the nearest layer
//    come the ground searchlights' beams (src/searchlights.rs): the beams
//    rise from our street, so they pass in front of the distant clouds
//    low in the sky and meet only the nearest layer overhead, which blocks
//    them and catches their pools of light. The
//    city, drawn later, hides the beams' feet;
// 3. the city: in Normal mode and during construction, the still city
//    (transparent where nothing is built yet); while Endless scrolls, the
//    four layers, far to near, at fractional positions;
// 4. the street's back layer (road and background trees), in world
//    columns, slid with the road camera like the nearest layer;
// 5. the vehicles, each at its own fractional position: every headlight
//    beam first, then every body, so a beam never covers the car ahead;
// 6. the street's front layer (foreground trees);
// 6b. rain or snow (src/weather.rs) in four layers far to near, in the
//    foreground over everything;
// 7. the message box, in the display-effects pass (post.wgsl), after the
//    palette's effect.
// Buildings hide the moon because they are drawn after it.
//
// Moving images are reconstructed as the spec describes: treat the image as
// a sharp device-resolution image (every cell cell x cell pixels) and give
// device pixel p the source pixels it overlaps at the fractional offset,
// decoded to linear light (the sRGB texture format decodes), premultiplied
// by alpha, and weighted by overlap. Blending in linear light keeps
// brightness constant while an edge moves by part of a pixel, so nothing
// flickers. The sharp image is never stored: its pixel x maps to cell
// floor(x / cell), so the small per-cell texture is sampled directly.

struct Params {
    // x: device pixels per cell; y: 1 when the Endless layers are drawn;
    // z: 1 when the still city is drawn; w: device pixels from the street
    // overlay's left edge to the screen's left edge (fractional; 0 in
    // Normal mode).
    cell: vec4<f32>,
    // Message box x, y, width, height in device pixels; width 0 hides it.
    message: vec4<f32>,
    // Per layer, x: device pixels from the layer buffer's left edge to the
    // screen's left edge (fractional). y, z, w unused.
    layers: array<vec4<f32>, 4>,
    // Moon: x, y device pixels from the sprite's top-left to the screen's
    // top-left (fractional); z: 1 when the moon is drawn; w unused.
    moon: vec4<f32>,
    // Per vehicle slot: x, y device pixels from the sprite's top-left to
    // the screen's top-left; z: 1 when the slot holds a vehicle.
    vehicles: array<vec4<f32>, 6>,
    // Cloud layers 0 (far) to 3 (near), each cross-fading between two
    // keyframes: device pixels from the front keyframe's strip to the
    // screen's left edge, the same for the back keyframe, the fade (weight
    // of the front), and the front slot (+2 if the back one exists; -1 if
    // the layer has nothing yet).
    cloud_front_offsets: vec4<f32>,
    cloud_back_offsets: vec4<f32>,
    cloud_fades: vec4<f32>,
    cloud_slots: vec4<f32>,
    // City glow: x the ground row in cells; y 1 on an ink-on-paper palette
    // (light effects drawn in ink, see `light`); z, w unused.
    glow: vec4<f32>,
    // Searchlights: rgb their color (linear), w how many are on.
    searchlight_color: vec4<f32>,
    // Per searchlight: x, y its foot in device pixels on screen; z its lean
    // from straight up (radians, positive right); w its power (0..1).
    searchlights: array<vec4<f32>, 3>,
    // Lightning: one light per flash of the current event: x, y its center
    // (device pixels), w its brightness (0..1; 0 when off).
    flash_lights: array<vec4<f32>, 3>,
    // x the bolt's segment count, y how much it lights the open sky, z the
    // bolt's brightness, w half the screen width (the light's reach).
    lightning_info: vec4<f32>,
    // The bolt's segments: x, y one end and z, w the other (device pixels).
    bolt: array<vec4<f32>, 24>,
    // Precipitation: x what falls (0 nothing, 1 rain, 2 snow), y how hard,
    // z, w the direction it falls (unit, y down).
    precip: vec4<f32>,
    // Per layer, far to near: x, y how far its tiles have moved (cells), z
    // the tile size (cells), w the share of tiles with a drop.
    precip_layers: array<vec4<f32>, 4>,
    // Per layer: x rain's streak length or snow's wobble (cells), y the
    // streak's half-width or the flake's size (cells; 2 is the spinning
    // cross), z opacity, w the clock (seconds) that spins the big flakes.
    precip_looks: array<vec4<f32>, 4>,
    // Special vehicles' lit flashers: x, y each center (device pixels), w
    // its brightness (0 when off); then each color (linear).
    flashers: array<vec4<f32>, 4>,
    flasher_colors: array<vec4<f32>, 4>,
    // A shooting star: x, y its head and z, w the end of its trail (device
    // pixels).
    meteor: vec4<f32>,
    // x its brightness (0 when there is none).
    meteor_info: vec4<f32>,
}

// Flashers light the nearest buildings (owner, 2026-11-05): how far, in
// cells, and how strongly.
const FLASHER_REACH: f32 = 22.0;
const FLASHER_STRENGTH: f32 = 0.35;

// A shooting star's streak half-width and soft edge, in cells.
const METEOR_CORE: f32 = 0.35;
const METEOR_EDGE: f32 = 0.9;

// The precipitation tile hash repeats every this many tiles, matching the
// wrap of each layer's travel (src/weather.rs, WRAP_TILES).
const PRECIP_WRAP: i32 = 4096;
// Rain is a little dimmer than the palette's star color; snow is the star
// color.
const RAIN_SHADE: f32 = 0.7;
// A comet drop's tail end, as a share of its head's brightness.
const RAIN_TAIL: f32 = 0.25;
// The big flakes' spin: radians per second at their slowest and the extra
// at their fastest (about one turn every 7-15 s), each flake clockwise or
// anticlockwise at random (owner, 2026-11-05).
const FLAKE_SPIN: f32 = 0.42;
const FLAKE_SPIN_EXTRA: f32 = 0.48;

// Lightning (src/lightning.rs): its color (a cool white, linear); how much
// it lights clouds near it from inside and the open sky around it; the
// bolt's bright core and soft glow, in cells.
const FLASH_COLOR: vec3<f32> = vec3<f32>(0.82, 0.88, 1.0);
const FLASH_CLOUD: f32 = 1.0;
const FLASH_SKY: f32 = 0.18;
// Each flash's light is a flat oval as wide as the screen, centred on the
// flash, fading smoothly to nothing at its edge (on screen or not), so
// several flashes layer over one another. Each starts at a quarter of full
// strength; overlaps add up (owner, 2026-11-05).
const FLASH_LIGHT: f32 = 0.25;
// The oval's height as a share of its width.
const FLASH_FLATNESS: f32 = 0.3;
// Pixel-art lightning (owner, 2026-11-05): the bolt is whole cells, one
// cell wide and stepping diagonally, with a one-cell ring of glow in a
// dimmer tone; the flash light is banded into tones with a dither between
// them, cell by cell.
const BOLT_GLOW_TONE: f32 = 0.35;
// Brightness bands the flash light falls into.
const FLASH_BANDS: f32 = 4.0;

// Searchlight beams: how fast they widen (radians each side) and how wide
// their foot is (cells); how bright the beam is in the air and how bright
// its pool on a cloud; how far it reaches (fading over this many heights
// of the sky); and how many points along the beam are checked for cloud
// that blocks it.
const BEAM_SPREAD: f32 = 0.035;
const BEAM_FOOT: f32 = 1.5;
const BEAM_AIR: f32 = 0.35;
const BEAM_POOL: f32 = 0.7;
const BEAM_REACH: f32 = 2.0;
const BEAM_STEPS: i32 = 10;
// How much of the light each step's cloud stops (cloud alpha times this).
const BEAM_BLOCK: f32 = 0.85;

// How strongly the city glow lights a cloud at the skyline.
const CITY_GLOW_STRENGTH: f32 = 1.0;
// The glow fades with height above the ground as exp(-height / this share
// of the sky's rows).
const CITY_GLOW_HEIGHT: f32 = 0.45;
// Rows below a cloud pixel checked for its underside, near and far: the
// more open sky below, the more the pixel is a base, lit most by the city.
// Two reaches make the glow a soft wash up from cloud bottoms rather than
// a line at every edge where one heap overlaps another.
const UNDERSIDE_NEAR: f32 = 3.0;
const UNDERSIDE_FAR: f32 = 8.0;
// How much each Endless layer (far to near) adds to the glow. Each layer
// is a whole city, and nearer buildings hide most windows behind them, so
// together they count about as much as one Normal skyline's visible
// windows.
const ENDLESS_GLOW_WEIGHTS: vec4<f32> = vec4<f32>(0.1, 0.15, 0.3, 0.75);
// Share of the glow a cloud's top and middle get (its base gets all).
const GLOW_ON_TOPS: f32 = 0.12;

// Cloud strips stacked in one texture: per layer (far at the top), two
// keyframe slots.
const CLOUD_LAYERS: u32 = 4u;
const CLOUD_SLOTS: u32 = 2u;

// Vehicle slots in the atlas; the atlas holds every beam sprite (rows
// 2 * slot) above every body sprite (rows 12 + 2 * slot).
const VEHICLE_SLOTS: u32 = 6u;
const VEHICLE_ROWS: i32 = 2;

@group(0) @binding(0) var background: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var message_box: texture_2d<f32>;
@group(0) @binding(3) var overlay: texture_2d<f32>;
@group(0) @binding(4) var layer0: texture_2d<f32>;
@group(0) @binding(5) var layer1: texture_2d<f32>;
@group(0) @binding(6) var layer2: texture_2d<f32>;
@group(0) @binding(7) var layer3: texture_2d<f32>;
@group(0) @binding(8) var moon_sprite: texture_2d<f32>;
@group(0) @binding(9) var still_city: texture_2d<f32>;
@group(0) @binding(10) var street_front: texture_2d<f32>;
@group(0) @binding(11) var vehicle_atlas: texture_2d<f32>;
@group(0) @binding(12) var cloud_strip: texture_2d<f32>;
@group(0) @binding(13) var city_glow: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(max(linear, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}

fn decode_srgb(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((max(encoded, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, encoded <= vec3<f32>(0.04045));
}

// The cell texel at (x, row), premultiplied; transparent outside the texture.
fn premultiplied(tex: texture_2d<f32>, x: i32, row: i32) -> vec4<f32> {
    let dims = vec2<i32>(textureDimensions(tex));
    if (x < 0 || row < 0 || x >= dims.x || row >= dims.y) {
        return vec4<f32>(0.0);
    }
    let texel = textureLoad(tex, vec2<i32>(x, row), 0);
    return vec4<f32>(texel.rgb * texel.a, texel.a);
}

// A moving image's linear, premultiplied color for this device pixel, with
// the screen's top-left `offset` device pixels into the image.
fn sharp_sample(tex: texture_2d<f32>, offset: vec2<f32>, position: vec2<f32>) -> vec4<f32> {
    let cell = params.cell.x;
    // The pixel's top-left corner in the image's sharp device-resolution form.
    let u = floor(position) + offset;
    let first = floor(u);
    let t = u - first;
    let c0 = vec2<i32>(floor(first / cell));
    let c1 = vec2<i32>(floor((first + 1.0) / cell));
    let top = mix(premultiplied(tex, c0.x, c0.y), premultiplied(tex, c1.x, c0.y), t.x);
    let bottom = mix(premultiplied(tex, c0.x, c1.y), premultiplied(tex, c1.x, c1.y), t.x);
    return mix(top, bottom, t.y);
}

// One vehicle sprite texel, premultiplied; transparent outside the slot.
fn vehicle_texel(x: i32, row: i32, base_row: i32) -> vec4<f32> {
    let width = i32(textureDimensions(vehicle_atlas).x);
    if (x < 0 || x >= width || row < 0 || row >= VEHICLE_ROWS) {
        return vec4<f32>(0.0);
    }
    return premultiplied(vehicle_atlas, x, base_row + row);
}

// A vehicle sprite (its beam or its body) at its fractional position,
// reconstructed like sharp_sample but confined to its atlas slot.
fn vehicle_sample(slot: u32, body: bool, position: vec2<f32>) -> vec4<f32> {
    let placement = params.vehicles[slot];
    if (placement.z < 0.5) {
        return vec4<f32>(0.0);
    }
    var base_row = i32(slot) * VEHICLE_ROWS;
    if (body) {
        base_row += i32(VEHICLE_SLOTS) * VEHICLE_ROWS;
    }
    let cell = params.cell.x;
    let u = floor(position) + placement.xy;
    let first = floor(u);
    let t = u - first;
    let c0 = vec2<i32>(floor(first / cell));
    let c1 = vec2<i32>(floor((first + 1.0) / cell));
    let top = mix(vehicle_texel(c0.x, c0.y, base_row), vehicle_texel(c1.x, c0.y, base_row), t.x);
    let bottom = mix(vehicle_texel(c0.x, c1.y, base_row), vehicle_texel(c1.x, c1.y, base_row), t.x);
    return mix(top, bottom, t.y);
}

// One cloud texel of strip `strip` (layer * CLOUD_SLOTS + slot),
// premultiplied; transparent outside that strip of the shared texture.
fn cloud_texel(x: i32, row: i32, strip: u32) -> vec4<f32> {
    let dims = vec2<i32>(textureDimensions(cloud_strip));
    let rows = dims.y / i32(CLOUD_LAYERS * CLOUD_SLOTS);
    if (x < 0 || x >= dims.x || row < 0 || row >= rows) {
        return vec4<f32>(0.0);
    }
    return premultiplied(cloud_strip, x, i32(strip) * rows + row);
}

// One cloud strip at its fractional offset, reconstructed like
// sharp_sample but confined to that strip of the texture.
fn cloud_sample(band: u32, offset: f32, position: vec2<f32>) -> vec4<f32> {
    let cell = params.cell.x;
    let u = floor(position) + vec2<f32>(offset, 0.0);
    let first = floor(u);
    let t = u - first;
    let c0 = vec2<i32>(floor(first / cell));
    let c1 = vec2<i32>(floor((first + 1.0) / cell));
    let top = mix(cloud_texel(c0.x, c0.y, band), cloud_texel(c1.x, c0.y, band), t.x);
    let bottom = mix(cloud_texel(c0.x, c1.y, band), cloud_texel(c1.x, c1.y, band), t.x);
    return mix(top, bottom, t.y);
}

// One city glow texel: column `x` of glow row `row`, black outside.
fn glow_texel(x: i32, row: i32) -> vec3<f32> {
    let dims = vec2<i32>(textureDimensions(city_glow));
    if (x < 0 || x >= dims.x || row < 0 || row >= dims.y) {
        return vec3<f32>(0.0);
    }
    return textureLoad(city_glow, vec2<i32>(x, row), 0).rgb;
}

// The city's glow under screen position `position` (linear RGB): from the
// still city, or the four Endless layers each at its own offset.
fn city_glow_at(position: vec2<f32>) -> vec3<f32> {
    let cell = params.cell.x;
    if (params.cell.z > 0.5) {
        return glow_texel(i32(floor(position.x / cell)), 0);
    }
    var glow = vec3<f32>(0.0);
    if (params.cell.y > 0.5) {
        for (var layer = 0; layer < 4; layer++) {
            let x = i32(floor((position.x + params.layers[layer].x) / cell));
            glow += glow_texel(x, 1 + layer) * ENDLESS_GLOW_WEIGHTS[layer];
        }
    }
    return glow;
}

// One cloud layer's keyframes, cross-faded (premultiplied, linear).
fn cloud_layer(band: u32, position: vec2<f32>) -> vec4<f32> {
    let slots = params.cloud_slots[band];
    let front = u32(slots) % 2u;
    let fade = params.cloud_fades[band];
    var cloud = cloud_sample(band * CLOUD_SLOTS + front, params.cloud_front_offsets[band], position) * fade;
    if (slots >= 2.0) {
        let back = 1u - front;
        cloud += cloud_sample(band * CLOUD_SLOTS + back, params.cloud_back_offsets[band], position) * (1.0 - fade);
    }
    return cloud;
}

// The first cloud layer near enough to meet the searchlights: only the
// nearest, the big heaps overhead. The others, including the low scud
// along the horizon, are behind the beams.
const BEAM_FIRST_BAND: u32 = 3u;

// The near cloud layers' combined coverage at `position` (0..1): the
// clouds a searchlight beam can run into.
fn cloud_cover(position: vec2<f32>) -> f32 {
    var clear = 1.0;
    for (var band = BEAM_FIRST_BAND; band < CLOUD_LAYERS; band++) {
        if (params.cloud_slots[band] >= 0.0) {
            clear *= 1.0 - cloud_layer(band, position).a;
        }
    }
    return 1.0 - clear;
}

// How much of searchlight `light`'s beam reaches `position`, 0..1: its
// cross-section (soft-edged, widening with distance), fading with
// distance, times its power. Cloud is not counted here.
fn beam_shape(light: vec4<f32>, position: vec2<f32>) -> f32 {
    if (light.w <= 0.0) {
        return 0.0;
    }
    let cell = params.cell.x;
    let direction = vec2<f32>(sin(light.z), -cos(light.z));
    let offset = position - light.xy;
    let along = dot(offset, direction);
    if (along <= 0.0) {
        return 0.0;
    }
    let across = abs(offset.x * direction.y - offset.y * direction.x);
    let half_width = 0.5 * BEAM_FOOT * cell + along * BEAM_SPREAD;
    // A soft (gaussian) cross-section: bright along the middle, fading to
    // nothing at the edges, like light scattered by the night air.
    let r = across / half_width;
    if (r > 1.6) {
        return 0.0;
    }
    let edge = exp(-2.0 * r * r);
    let sky = max(params.glow.x, 1.0) * cell;
    return edge * exp(-along / (BEAM_REACH * sky)) * light.w;
}

// The share of searchlight `light`'s beam that gets past the clouds
// between its foot and `position` (not counting the cloud at `position`).
fn beam_through(light: vec4<f32>, position: vec2<f32>) -> f32 {
    var through = 1.0;
    for (var step = 1; step <= BEAM_STEPS; step++) {
        let point = mix(light.xy, position, f32(step) / f32(BEAM_STEPS + 1));
        through *= 1.0 - BEAM_BLOCK * cloud_cover(point);
    }
    return through;
}

// How far (in cells, measured square: the larger of across and down) the
// cell whose center is `cell_center` (cells) is from the nearest bolt
// segment. Cells within half a cell make a one-cell-wide stepped line,
// like a pixel-art line.
fn bolt_cells(cell_center: vec2<f32>) -> f32 {
    var nearest = 1e9;
    let count = u32(params.lightning_info.x);
    let cell = params.cell.x;
    for (var i = 0u; i < min(count, 24u); i++) {
        let segment = params.bolt[i] / cell;
        let a = segment.xy;
        let ab = segment.zw - a;
        let t = clamp(dot(cell_center - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
        let d = abs(cell_center - (a + ab * t));
        nearest = min(nearest, max(d.x, d.y));
    }
    return nearest;
}

// The 4x4 Bayer threshold (0..1) for cell `cell`, for dithering between
// flash light bands.
fn bayer(cell: vec2<i32>) -> f32 {
    let m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let x = u32(cell.x) & 3u;
    let y = u32(cell.y) & 3u;
    return (m[y * 4u + x] + 0.5) / 16.0;
}

// Three seeded numbers in 0..1 for precipitation tile `id` of layer
// `layer`.
fn precip_hash(id: vec2<i32>, layer: u32) -> vec3<f32> {
    var h = (u32(id.x) * 0x8da6b343u) ^ (u32(id.y) * 0xd8163841u) ^ ((layer + 1u) * 0xcb1ab31fu);
    h ^= h >> 13u;
    h *= 0x5bd1e995u;
    h ^= h >> 15u;
    let b = (h * 0x27d4eb2du) ^ (h >> 16u);
    let c = (b * 0x165667b1u) ^ (b >> 15u);
    return vec3<f32>(f32(h & 0xffffu), f32(b & 0xffffu), f32(c & 0xffffu)) / 65535.0;
}

// How much of precipitation layer `layer` covers the cell at `cell_pos`
// (cell units, the cell's center), 0..1. Each tile of the moving grid holds
// at most one drop: a rain streak along the fall, or a wobbling flake.
fn precip_alpha(layer: u32, cell_pos: vec2<f32>, pixel: vec2<f32>) -> f32 {
    let grid = params.precip_layers[layer];
    let look = params.precip_looks[layer];
    if (grid.w <= 0.0) {
        return 0.0;
    }
    let tile = max(grid.z, 1.0);
    let q = cell_pos - grid.xy;
    let base = vec2<i32>(floor(q / tile));
    let direction = params.precip.zw;
    let snow = params.precip.x > 1.5;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let id = base + vec2<i32>(dx, dy);
            let wrapped = ((id % PRECIP_WRAP) + PRECIP_WRAP) % PRECIP_WRAP;
            let h = precip_hash(wrapped, layer);
            if (h.x >= grid.w) {
                continue;
            }
            let center = (vec2<f32>(id) + vec2<f32>(0.15 + 0.7 * h.y, 0.15 + 0.7 * h.z)) * tile;
            var offset = q - center;
            if (snow) {
                // A flake drifts side to side as it falls down the screen.
                // Its size (owner, 2026-11-05): the nearest a 5-cell X, then
                // one cell, then smaller than a cell, drawn in screen pixels.
                let wobble = sin(cell_pos.y * 0.18 + h.y * 6.2832) * look.x;
                let flake = center + grid.xy + vec2<f32>(wobble, 0.0);
                let from_flake = floor(cell_pos) - floor(flake);
                if (look.y > 1.5) {
                    // The big flakes move smoothly and spin slowly (owner,
                    // 2026-11-05): two crossed bars, 3 cells long and one
                    // thick (an X or a + as they turn), drawn in screen
                    // pixels at their exact place, their edges blended
                    // over a pixel so motion never steps or flickers. The
                    // wobble follows the flake's own height, not the cell
                    // being drawn, so it is smooth too.
                    let cell = params.cell.x;
                    let smooth_wobble = sin((center.y + grid.y) * 0.18 + h.y * 6.2832) * look.x;
                    let middle = (center + grid.xy + vec2<f32>(smooth_wobble, 0.0)) * cell;
                    let turn = select(-1.0, 1.0, h.z < 0.5) * (FLAKE_SPIN + FLAKE_SPIN_EXTRA * h.y);
                    let angle = h.x * 6.2832 + turn * look.w;
                    let c = cos(angle);
                    let s = sin(angle);
                    let rel = pixel + vec2<f32>(0.5) - middle;
                    let r = vec2<f32>(c * rel.x + s * rel.y, -s * rel.x + c * rel.y);
                    let arm = vec2<f32>(1.5, 0.5) * cell;
                    let bar1 = max(abs(r.x) - arm.x, abs(r.y) - arm.y);
                    let bar2 = max(abs(r.y) - arm.x, abs(r.x) - arm.y);
                    let coverage = clamp(0.5 - min(bar1, bar2), 0.0, 1.0);
                    if (coverage > 0.0) {
                        return look.z * coverage;
                    }
                } else if (look.y >= 1.0) {
                    if (all(from_flake == vec2<f32>(0.0))) {
                        return look.z;
                    }
                } else {
                    let cell = params.cell.x;
                    let size = max(1.0, round(look.y * cell));
                    let corner = floor(flake * cell);
                    if (all(pixel >= corner) && all(pixel < corner + vec2<f32>(size))) {
                        return look.z;
                    }
                }
            } else {
                // A comet drop (owner, 2026-11-05): one cell wide, its head
                // (the end it falls toward) brightest and its tail fading
                // behind it.
                let along = dot(offset, direction);
                let across = abs(offset.x * direction.y - offset.y * direction.x);
                if (abs(along) <= 0.5 * look.x && across <= look.y) {
                    let toward_head = along / look.x + 0.5;
                    return look.z * (RAIN_TAIL + (1.0 - RAIN_TAIL) * toward_head * toward_head);
                }
            }
        }
    }
    return 0.0;
}

// Adds `amount` of the light `tint` to `color`; on an ink-on-paper palette
// (Ink on Parchment), where light would vanish against the pale sky, draws
// it in the palette's ink instead: a stroke `amount` strong (owner,
// 2026-11-05).
fn light(color: vec3<f32>, tint: vec3<f32>, amount: f32) -> vec3<f32> {
    if (params.glow.y > 0.5) {
        return mix(color, params.searchlight_color.rgb, clamp(amount, 0.0, 1.0));
    }
    return color + tint * amount;
}

fn over(front: vec4<f32>, back: vec3<f32>) -> vec3<f32> {
    return front.rgb + back * (1.0 - front.a);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let cell = params.cell.x;
    let texel = vec2<i32>(floor(position.xy / cell));
    let dims = vec2<i32>(textureDimensions(background));
    var color = vec3<f32>(0.0);
    if (all(texel < dims) && all(texel >= vec2<i32>(0, 0))) {
        color = textureLoad(background, texel, 0).rgb;
    }

    if (params.moon.z > 0.5) {
        color = over(sharp_sample(moon_sprite, params.moon.xy, position.xy), color);
    }
    // A shooting star (src/ambience.rs), behind the clouds: brightest at
    // its head, its trail fading behind it.
    if (params.meteor_info.x > 0.0) {
        let head = params.meteor.xy;
        let trail = params.meteor.zw - head;
        let t = clamp(dot(position.xy - head, trail) / max(dot(trail, trail), 1e-6), 0.0, 1.0);
        let d = length(position.xy - (head + trail * t)) / cell;
        let along = 1.0 - t;
        let streak = 1.0 - smoothstep(METEOR_CORE, METEOR_EDGE, d);
        color = light(color, params.searchlight_color.rgb, params.meteor_info.x * streak * along * along);
    }
    // The cloud layers far to near, each at its own fractional offset, lit
    // (including by the moon) on a background thread; they let some of the
    // moon and stars behind them through.
    // Each layer cross-fades (in linear light, premultiplied) from its back
    // keyframe to its front one. The city glow then lights it: strongest on
    // undersides (less cloud just below) and low clouds, fading with height.
    // The searchlights: each beam's light here, past the clouds below.
    var beam = 0.0;
    let lights = u32(params.searchlight_color.w);
    for (var slot = 0u; slot < min(lights, 3u); slot++) {
        let light = params.searchlights[slot];
        let shape = beam_shape(light, position.xy);
        if (shape > 0.002) {
            beam += shape * beam_through(light, position.xy);
        }
    }
    let beam_light = params.searchlight_color.rgb * beam;
    // Lightning: how bright it is here (fading with distance from its
    // center), lighting the open sky a little and the clouds from inside.
    let flash = params.lightning_info.z;
    var flash_here = 0.0;
    let reach = vec2<f32>(1.0, FLASH_FLATNESS) * max(params.lightning_info.w, 1.0);
    // Worked out once per cell, at its center, so the light is blocky.
    let cell_center = (floor(position.xy / cell) + vec2<f32>(0.5)) * cell;
    for (var i = 0u; i < 3u; i++) {
        let light = params.flash_lights[i];
        if (light.w > 0.0) {
            let offset = (cell_center - light.xy) / reach;
            let q = dot(offset, offset);
            if (q < 1.0) {
                flash_here += light.w * FLASH_LIGHT * (1.0 - q) * (1.0 - q);
            }
        }
    }
    if (flash_here > 0.0) {
        // Banded into tones, dithered between bands (a cell takes the next
        // band up when its share past this one beats its Bayer threshold).
        let peak = FLASH_LIGHT;
        let level = clamp(flash_here / peak, 0.0, 4.0) * FLASH_BANDS;
        let band = floor(level) + select(0.0, 1.0, fract(level) > bayer(vec2<i32>(floor(position.xy / cell))));
        flash_here = band / FLASH_BANDS * peak;
    }
    if (flash_here > 0.0) {
        color += FLASH_COLOR * flash_here * FLASH_SKY * params.lightning_info.y;
    }
    let glow = city_glow_at(position.xy);
    let ground = params.glow.x;
    let height = max(ground - position.y / cell, 0.0);
    let glow_light = glow * CITY_GLOW_STRENGTH * exp(-height / (CITY_GLOW_HEIGHT * max(ground, 1.0)));
    let near_below = position.xy + vec2<f32>(0.0, UNDERSIDE_NEAR * cell);
    let far_below = position.xy + vec2<f32>(0.0, UNDERSIDE_FAR * cell);
    for (var band = 0u; band < CLOUD_LAYERS; band++) {
        if (band == BEAM_FIRST_BAND) {
            color = light(color, params.searchlight_color.rgb, beam * BEAM_AIR);
        }
        if (params.cloud_slots[band] < 0.0) {
            continue;
        }
        var cloud = cloud_layer(band, position.xy);
        if (cloud.a > 0.0 && any(glow_light > vec3<f32>(0.0))) {
            let open_below = 1.0 - 0.5 * (cloud_layer(band, near_below).a + cloud_layer(band, far_below).a);
            let underside = clamp(open_below, 0.0, 1.0);
            cloud = vec4<f32>(cloud.rgb + glow_light * cloud.a * mix(GLOW_ON_TOPS, 1.0, underside), cloud.a);
        }
        // A searchlight's pool where its beam meets this cloud.
        if (band >= BEAM_FIRST_BAND && cloud.a > 0.0 && beam > 0.0) {
            let lit = light(cloud.rgb, params.searchlight_color.rgb * cloud.a, beam * BEAM_POOL);
            cloud = vec4<f32>(select(lit, mix(cloud.rgb, params.searchlight_color.rgb * cloud.a, beam * BEAM_POOL), params.glow.y > 0.5), cloud.a);
        }
        if (flash_here > 0.0 && cloud.a > 0.0) {
            cloud = vec4<f32>(cloud.rgb + FLASH_COLOR * flash_here * FLASH_CLOUD * cloud.a, cloud.a);
        }
        color = over(cloud, color);
    }
    // The bolt, in front of the clouds and behind the city: whole cells,
    // a one-cell-wide stepped line at full brightness and a one-cell ring
    // of glow around it in a dimmer tone.
    if (flash > 0.0 && params.lightning_info.x > 0.0) {
        let d = bolt_cells(floor(position.xy / cell) + vec2<f32>(0.5));
        if (d <= 0.5) {
            let ink = params.glow.y > 0.5;
            color = mix(color, select(FLASH_COLOR, params.searchlight_color.rgb, ink), flash);
        } else if (d <= 1.5) {
            color = light(color, FLASH_COLOR, flash * BOLT_GLOW_TONE);
        }
    }
    // How much of this pixel is a near building, for flasher light.
    var building = 0.0;
    if (params.cell.z > 0.5) {
        let still = premultiplied(still_city, texel.x, texel.y);
        color = over(still, color);
        building = still.a;
    }
    if (params.cell.y > 0.5) {
        let p = position.xy;
        color = over(sharp_sample(layer0, vec2<f32>(params.layers[0].x, 0.0), p), color);
        color = over(sharp_sample(layer1, vec2<f32>(params.layers[1].x, 0.0), p), color);
        let near_back = sharp_sample(layer2, vec2<f32>(params.layers[2].x, 0.0), p);
        let near_front = sharp_sample(layer3, vec2<f32>(params.layers[3].x, 0.0), p);
        color = over(near_back, color);
        color = over(near_front, color);
        building = max(near_back.a, near_front.a);
    }
    // Flashers light the nearest buildings in their colors, fading with
    // distance.
    if (building > 0.0) {
        for (var i = 0u; i < 4u; i++) {
            let light = params.flashers[i];
            if (light.w > 0.0) {
                let d = length(position.xy - light.xy) / (cell * FLASHER_REACH);
                if (d < 1.0) {
                    let fall = (1.0 - d * d) * (1.0 - d * d);
                    color += params.flasher_colors[i].rgb * light.w * FLASHER_STRENGTH * fall * building;
                }
            }
        }
    }
    let street_offset = vec2<f32>(params.cell.w, 0.0);
    color = over(sharp_sample(overlay, street_offset, position.xy), color);
    for (var slot = 0u; slot < VEHICLE_SLOTS; slot++) {
        color = over(vehicle_sample(slot, false, position.xy), color);
    }
    for (var slot = 0u; slot < VEHICLE_SLOTS; slot++) {
        color = over(vehicle_sample(slot, true, position.xy), color);
    }
    color = over(sharp_sample(street_front, street_offset, position.xy), color);

    // Rain or snow in the foreground, far layers first, drawn whole cells
    // at a time like the rest of the pixel art.
    if (params.precip.x > 0.5) {
        let shade = select(RAIN_SHADE, 1.0, params.precip.x > 1.5);
        let drop_color = params.searchlight_color.rgb * shade;
        let cell_pos = floor(position.xy / cell) + vec2<f32>(0.5);
        for (var layer = 0u; layer < 4u; layer++) {
            color = mix(color, drop_color, precip_alpha(layer, cell_pos, floor(position.xy)));
        }
    }

    // The message box is drawn by the display-effects pass (post.wgsl), over
    // any effect.
    return vec4<f32>(color, 1.0);
}
