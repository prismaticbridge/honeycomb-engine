struct AffineTransform {
    col0: vec2<f32>,
    col1: vec2<f32>,
    translation: vec2<f32>,
}
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec3<f32>,
};
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec3<f32>,
};

@group(0) @binding(0)
var<uniform> camera: AffineTransform;

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let view_pos = vertex.position + camera.translation;
    let clip_pos = camera.col0 * view_pos.x + camera.col1 * view_pos.y;
    out.clip_position = vec4<f32>(clip_pos, 0.0, 1.0);
    out.color = vec3<f32>(vertex.color);
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    //no srgb conversion? maybe? who knows
    let linear = srgb_to_linear(in.color);
    return vec4(linear, 1.0);
    // return vec4(in.color, 1.0);
}
