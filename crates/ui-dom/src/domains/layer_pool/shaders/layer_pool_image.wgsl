struct RectPipelineUniform {
    target_size_px: vec4<f32>,
    layer_zero_in_image_px: vec4<f32>,
    density_scale_px: vec4<f32>,
};

struct VertexInput {
    @location(0) position_px: vec2<f32>,
    @location(1) size_px: vec2<f32>,
    @location(2) fill_rgba: vec4<f32>,
    @builtin(vertex_index) vertex_index: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) fill_rgba: vec4<f32>,
};

@group(0) @binding(0) var<uniform> rect_pipeline: RectPipelineUniform;

fn quad_position(vertex_index: u32) -> vec2<f32> {
    switch vertex_index {
        case 0u { return vec2<f32>(0.0, 0.0); }
        case 1u { return vec2<f32>(1.0, 0.0); }
        case 2u { return vec2<f32>(0.0, 1.0); }
        case 3u { return vec2<f32>(0.0, 1.0); }
        case 4u { return vec2<f32>(1.0, 0.0); }
        default { return vec2<f32>(1.0, 1.0); }
    }
}

fn clip_x(value_px: f32) -> f32 {
    return ((value_px / rect_pipeline.target_size_px.x) * 2.0) - 1.0;
}

fn clip_y(value_px: f32) -> f32 {
    return 1.0 - ((value_px / rect_pipeline.target_size_px.y) * 2.0);
}

@vertex
fn vertex_main(input: VertexInput) -> VertexOutput {
    let corner = quad_position(input.vertex_index);
    let density_scale = rect_pipeline.density_scale_px.x;
    let local_pos_px = (corner * input.size_px) * density_scale;
    let world_px = (input.position_px * density_scale) + local_pos_px;
    var output: VertexOutput;
    output.clip_position = vec4<f32>(clip_x(world_px.x), clip_y(world_px.y), 0.0, 1.0);
    output.fill_rgba = input.fill_rgba;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.fill_rgba;
}
