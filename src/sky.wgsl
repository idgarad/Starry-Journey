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
    let vertical = clamp(position.y / 180.0, 0.0, 1.0);
    let upper_sky = vec3<f32>(0.035, 0.075, 0.13);
    let middle_sky = vec3<f32>(0.075, 0.15, 0.24);
    let horizon_sky = vec3<f32>(0.20, 0.25, 0.32);
    let upper_gradient = mix(upper_sky, middle_sky, smoothstep(0.0, 0.72, vertical));
    let sky = mix(upper_gradient, horizon_sky, smoothstep(0.72, 1.0, vertical));
    return vec4<f32>(sky, 1.0);
}
