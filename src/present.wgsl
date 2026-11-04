// Presents the scene. Every texture holds one texel per logical cell (the
// message box excepted); each cell covers a cell x cell block of device
// pixels from the top-left.
//
// Draw order (Rebuild Prompt, "Final draw order"), in linear light:
// 1. background: the sky and stars;
// 2. the moon (or battle station), at its exact fractional position;
// 3. the city: in Normal mode and during construction, the still city
//    (transparent where nothing is built yet); while Endless scrolls, the
//    four layers, far to near, at fractional positions;
// 4. the overlay: street, trees, and traffic;
// 5. the message box.
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
    // z: 1 when the still city is drawn; w unused (16-byte alignment).
    cell: vec4<f32>,
    // Message box x, y, width, height in device pixels; width 0 hides it.
    message: vec4<f32>,
    // Per layer, x: device pixels from the layer buffer's left edge to the
    // screen's left edge (fractional). y, z, w unused.
    layers: array<vec4<f32>, 4>,
    // Moon: x, y device pixels from the sprite's top-left to the screen's
    // top-left (fractional); z: 1 when the moon is drawn; w unused.
    moon: vec4<f32>,
}

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
    color = over(premultiplied(overlay, texel.x, texel.y), color);

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
