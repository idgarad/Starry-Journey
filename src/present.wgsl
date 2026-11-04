// Presents the logical city buffer: every logical cell becomes an exact
// cell x cell block of device pixels, anchored at the top-left corner.
// Cells past the buffer edge (the grid may overrun the surface by less
// than one cell) are simply clipped by the surface.
//
// The message box is drawn over the city at device resolution. Its texture
// holds premultiplied, sRGB-encoded RGBA and is blended in sRGB space, like
// a browser's 75%-opacity box, then decoded for the sRGB surface.

struct Params {
    // x: device pixels per logical cell; y, z, w unused (16-byte alignment).
    cell: vec4<f32>,
    // Message box x, y, width, height in device pixels; width 0 hides it.
    message: vec4<f32>,
}

@group(0) @binding(0) var city: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var message_box: texture_2d<f32>;

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

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(city));
    let texel = vec2<i32>(floor(position.xy / params.cell.x));
    var color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if (all(texel < dims) && all(texel >= vec2<i32>(0, 0))) {
        color = textureLoad(city, texel, 0);
    }

    let local = vec2<i32>(floor(position.xy - params.message.xy));
    let size = vec2<i32>(params.message.zw);
    if (params.message.z > 0.0 && all(local >= vec2<i32>(0, 0)) && all(local < size)) {
        let overlay = textureLoad(message_box, local, 0);
        let blended = overlay.rgb + encode_srgb(color.rgb) * (1.0 - overlay.a);
        color = vec4<f32>(decode_srgb(blended), 1.0);
    }
    return color;
}
