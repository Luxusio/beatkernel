struct Playfield {
    // logical width, height, local epoch drift in pixels, reserved
    viewport: vec4<f32>,
    // inclusive note-center clip top and bottom, reserved
    clip: vec4<f32>,
};
@group(0) @binding(0) var<uniform> field: Playfield;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex: u32,
    @location(0) geometry: vec4<f32>,
    @location(1) appearance: vec4<f32>,
) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let head = geometry.z + field.viewport.z;
    let tail = geometry.w + field.viewport.z;
    var top = max(tail, field.clip.x);
    var bottom = max(top, min(head, field.clip.y));
    if appearance.x > 0.5 {
        var center = tail;
        var half_height = 3.0;
        if appearance.x > 1.5 {
            center = head;
            half_height = 4.0;
        }
        top = center - half_height;
        bottom = center + half_height;
        if center < field.clip.x || center > field.clip.y {
            bottom = top;
        }
    }
    // Off-screen heads have zero-area triangles; bodies are clipped before
    // interpolation. Horizontal geometry was admitted by the scene composer.
    let pixel = vec2<f32>(geometry.x, top)
        + corners[vertex] * vec2<f32>(geometry.y, bottom - top);
    var output: VertexOutput;
    output.position = vec4<f32>(
        pixel.x / field.viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / field.viewport.y * 2.0,
        0.0, 1.0,
    );
    output.color = vec4<f32>(appearance.yzw, 1.0);
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
