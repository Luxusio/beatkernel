struct Viewport {
    size: vec2<f32>,
    translation: vec2<f32>,
};
@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(0) @binding(1) var sprite: texture_2d<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;
struct Component {
    offset_scale: vec4<f32>,
    pivot_opacity: vec4<f32>,
    clip: vec4<f32>,
    source_clip: vec4<f32>,
};
@group(1) @binding(0) var<uniform> component: Component;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) local_pixel: vec2<f32>,
    @location(3) source_pixel: vec2<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex: u32,
    @location(0) bounds: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec4<f32>,
) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let source = bounds.xy + corners[vertex] * bounds.zw;
    let local_pixel = (source - component.pivot_opacity.xy) * component.offset_scale.zw
        + component.pivot_opacity.xy + component.offset_scale.xy;
    let pixel = local_pixel + viewport.translation;
    var output: VertexOutput;
    output.position = vec4<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
        0.0, 1.0,
    );
    output.color = vec4<f32>(color.rgb, color.a * component.pivot_opacity.z);
    output.local_pixel = local_pixel;
    output.source_pixel = source;
    output.uv = uv.xy + corners[vertex] * uv.zw;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.source_pixel.x < component.source_clip.x || input.source_pixel.y < component.source_clip.y
        || input.source_pixel.x >= component.source_clip.z || input.source_pixel.y >= component.source_clip.w {
        discard;
    }
    if input.local_pixel.x < component.clip.x || input.local_pixel.y < component.clip.y
        || input.local_pixel.x >= component.clip.z || input.local_pixel.y >= component.clip.w {
        discard;
    }
    return textureSample(sprite, sprite_sampler, input.uv) * input.color;
}
