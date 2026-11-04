// Presents the scene. Every texture holds one texel per logical cell (the
// message box excepted); each cell covers a cell x cell block of device
// pixels from the top-left.
//
// Draw order (Rebuild Prompt, "Final draw order"), in linear light:
// 1. background: the sky and stars;
// 2. the moon (or battle station), at its exact fractional position;
// 2b. the procedural clouds (src/skyclouds.rs), lit on a background
//    thread, slid with the wind and (in Endless) a tenth of the camera, in
//    front of the moon and behind the city;
// 3. the city: in Normal mode and during construction, the still city
//    (transparent where nothing is built yet); while Endless scrolls, the
//    four layers, far to near, at fractional positions;
// 4. the street's back layer (road and background trees), in world
//    columns, slid with the road camera like the nearest layer;
// 5. the vehicles, each at its own fractional position: every headlight
//    beam first, then every body, so a beam never covers the car ahead;
// 6. the street's front layer (foreground trees);
// 7. the message box.
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
    // Clouds: x device pixels from the strip's left edge to the screen's
    // left edge (fractional); z: 1 when there are clouds.
    clouds: vec4<f32>,
}

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
    // The clouds, lit (including by the moon) on a background thread;
    // they let some of the moon and stars behind them through.
    if (params.clouds.z > 0.5) {
        color = over(sharp_sample(cloud_strip, vec2<f32>(params.clouds.x, 0.0), position.xy), color);
    }
    if (params.cell.z > 0.5) {
        color = over(premultiplied(still_city, texel.x, texel.y), color);
    }
    if (params.cell.y > 0.5) {
        let p = position.xy;
        color = over(sharp_sample(layer0, vec2<f32>(params.layers[0].x, 0.0), p), color);
        color = over(sharp_sample(layer1, vec2<f32>(params.layers[1].x, 0.0), p), color);
        color = over(sharp_sample(layer2, vec2<f32>(params.layers[2].x, 0.0), p), color);
        color = over(sharp_sample(layer3, vec2<f32>(params.layers[3].x, 0.0), p), color);
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

    // The message box is premultiplied sRGB, blended in sRGB like a browser.
    let local = vec2<i32>(floor(position.xy - params.message.xy));
    let size = vec2<i32>(params.message.zw);
    if (params.message.z > 0.0 && all(local >= vec2<i32>(0, 0)) && all(local < size)) {
        let box_color = textureLoad(message_box, local, 0);
        let blended = box_color.rgb + encode_srgb(color) * (1.0 - box_color.a);
        color = decode_srgb(blended);
    }
    return vec4<f32>(color, 1.0);
}
