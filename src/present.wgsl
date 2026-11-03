// Presents the logical city buffer: every logical cell becomes an exact
// cell x cell block of device pixels, anchored at the top-left corner.
// Cells past the buffer edge (the grid may overrun the surface by less
// than one cell) are simply clipped by the surface.

struct Params {
    // x: device pixels per logical cell; y, z, w unused (16-byte alignment).
    cell: vec4<f32>,
}

@group(0) @binding(0) var city: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(city));
    let texel = vec2<i32>(floor(position.xy / params.cell.x));
    if (any(texel >= dims) || any(texel < vec2<i32>(0, 0))) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return textureLoad(city, texel, 0);
}
