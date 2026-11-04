// Presents the scene. Every texture holds one texel per logical cell; each
// cell is drawn as an exact cell x cell block of device pixels from the
// top-left. Cells past a texture's edge are clipped.
//
// Draw order (Rebuild Prompt, "Final draw order"):
// 1. background: the whole Normal / construction frame, or in Endless
//    scrolling the sky, stars, and moon;
// 2. the four Endless city layers, far to near, at fractional positions;
// 3. the overlay (Endless street, trees, and traffic);
// 4. the message box.
//
// Endless layers are reconstructed as the spec describes: treat each layer
// as a sharp device-resolution image (every cell cell pixels wide), and
// give device pixel p the two source pixels it overlaps at fractional
// offset, decoded to linear light (the sRGB texture format decodes),
// premultiplied by alpha, and weighted by overlap. Blending in linear light
// keeps brightness constant while an edge moves by part of a pixel, so
// nothing flickers. The sharp image is never stored: its pixel x maps to
// cell floor(x / cell), so the small per-cell texture is sampled directly.

struct Params {
    // x: device pixels per cell; y: 1 when the Endless layers and overlay
    // are drawn; z, w unused (16-byte alignment).
    cell: vec4<f32>,
    // Message box x, y, width, height in device pixels; width 0 hides it.
    message: vec4<f32>,
    // Per layer, x: device pixels from the layer buffer's left edge to the
    // screen's left edge (fractional). y, z, w unused.
    layers: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var background: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var message_box: texture_2d<f32>;
@group(0) @binding(3) var overlay: texture_2d<f32>;
@group(0) @binding(4) var layer0: texture_2d<f32>;
@group(0) @binding(5) var layer1: texture_2d<f32>;
@group(0) @binding(6) var layer2: texture_2d<f32>;
@group(0) @binding(7) var layer3: texture_2d<f32>;

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

// One layer's linear, premultiplied color for this device pixel.
fn layer_sample(tex: texture_2d<f32>, offset: f32, position: vec2<f32>) -> vec4<f32> {
    let cell = params.cell.x;
    let row = i32(floor(position.y / cell));
    // The pixel's left edge in the layer's sharp device-resolution image.
    let u = floor(position.x) + offset;
    let left = floor(u);
    let t = u - left;
    let a = premultiplied(tex, i32(floor(left / cell)), row);
    let b = premultiplied(tex, i32(floor((left + 1.0) / cell)), row);
    return mix(a, b, t);
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

    if (params.cell.y > 0.5) {
        color = over(layer_sample(layer0, params.layers[0].x, position.xy), color);
        color = over(layer_sample(layer1, params.layers[1].x, position.xy), color);
        color = over(layer_sample(layer2, params.layers[2].x, position.xy), color);
        color = over(layer_sample(layer3, params.layers[3].x, position.xy), color);
        color = over(premultiplied(overlay, texel.x, texel.y), color);
    }

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
