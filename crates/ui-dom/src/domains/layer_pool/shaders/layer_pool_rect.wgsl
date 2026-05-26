struct RectPipelineUniform {
    target_size_px: vec4<f32>,
    layer_zero_in_image_px: vec4<f32>,
    density_scale_px: vec4<f32>,
};

struct VertexInput {
    @location(0) position_px: vec2<f32>,
    @location(1) size_px: vec2<f32>,
    @location(2) fill_rgba: vec4<f32>,
    @location(3) corner_radii_px: vec4<f32>,
    @location(4) border_widths_px: vec4<f32>,
    @location(5) border_top_rgba: vec4<f32>,
    @location(6) border_right_rgba: vec4<f32>,
    @location(7) border_bottom_rgba: vec4<f32>,
    @location(8) border_left_rgba: vec4<f32>,
    @builtin(vertex_index) vertex_index: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos_px: vec2<f32>,
    @location(1) size_px: vec2<f32>,
    @location(2) fill_rgba: vec4<f32>,
    @location(3) corner_radii_px: vec4<f32>,
    @location(4) border_widths_px: vec4<f32>,
    @location(5) border_top_rgba: vec4<f32>,
    @location(6) border_right_rgba: vec4<f32>,
    @location(7) border_bottom_rgba: vec4<f32>,
    @location(8) border_left_rgba: vec4<f32>,
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
    let scaled_size_px = input.size_px * density_scale;
    let local_pos_px = corner * scaled_size_px;
    let world_px = (input.position_px * density_scale) + local_pos_px;
    var output: VertexOutput;
    output.clip_position = vec4<f32>(clip_x(world_px.x), clip_y(world_px.y), 0.0, 1.0);
    output.local_pos_px = local_pos_px;
    output.size_px = scaled_size_px;
    output.fill_rgba = input.fill_rgba;
    output.corner_radii_px = input.corner_radii_px * density_scale;
    output.border_widths_px = input.border_widths_px * density_scale;
    output.border_top_rgba = input.border_top_rgba;
    output.border_right_rgba = input.border_right_rgba;
    output.border_bottom_rgba = input.border_bottom_rgba;
    output.border_left_rgba = input.border_left_rgba;
    return output;
}

fn clamped_corner_radii(size_px: vec2<f32>, radii_px: vec4<f32>) -> vec4<f32> {
    let max_radius = min(size_px.x, size_px.y) * 0.5;
    return clamp(radii_px, vec4<f32>(0.0), vec4<f32>(max_radius));
}

fn corner_radius_for_local(local_pos_px: vec2<f32>, size_px: vec2<f32>, radii_px: vec4<f32>) -> f32 {
    let left_half = local_pos_px.x <= size_px.x * 0.5;
    let top_half = local_pos_px.y <= size_px.y * 0.5;
    if left_half {
        if top_half {
            return radii_px.x;
        }
        return radii_px.w;
    }
    if top_half {
        return radii_px.y;
    }
    return radii_px.z;
}

fn rounded_rect_alpha(local_pos_px: vec2<f32>, size_px: vec2<f32>, radii_px: vec4<f32>) -> f32 {
    let clamped_radii = clamped_corner_radii(size_px, radii_px);
    let radius_px = corner_radius_for_local(local_pos_px, size_px, clamped_radii);
    let half_size = size_px * 0.5;
    let center = local_pos_px - half_size;
    let q = abs(center) - (half_size - vec2<f32>(radius_px, radius_px));
    let outside = length(max(q, vec2<f32>(0.0, 0.0)));
    let inside = min(max(q.x, q.y), 0.0);
    let signed_distance = outside + inside - radius_px;
    return 1.0 - smoothstep(-1.0, 1.0, signed_distance);
}

fn inner_corner_radii(outer_radii_px: vec4<f32>, border_widths_px: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(
        max(outer_radii_px.x - max(border_widths_px.x, border_widths_px.w), 0.0),
        max(outer_radii_px.y - max(border_widths_px.x, border_widths_px.y), 0.0),
        max(outer_radii_px.z - max(border_widths_px.z, border_widths_px.y), 0.0),
        max(outer_radii_px.w - max(border_widths_px.z, border_widths_px.w), 0.0),
    );
}

fn corner_blend(top_or_bottom_rgba: vec4<f32>, left_or_right_rgba: vec4<f32>, major_px: f32, minor_px: f32) -> vec4<f32> {
    let total_px = max(major_px + minor_px, 0.0001);
    let top_or_bottom_weight = major_px / total_px;
    let left_or_right_weight = minor_px / total_px;
    return (top_or_bottom_rgba * top_or_bottom_weight) + (left_or_right_rgba * left_or_right_weight);
}

fn corner_radius_for_edges(top_edge: bool, left_edge: bool, radii_px: vec4<f32>) -> f32 {
    if left_edge {
        if top_edge {
            return radii_px.x;
        }
        return radii_px.w;
    }
    if top_edge {
        return radii_px.y;
    }
    return radii_px.z;
}

fn rgba_over_with_coverage(bottom_rgba: vec4<f32>, top_rgba: vec4<f32>, top_coverage: f32) -> vec4<f32> {
    let top_alpha = top_rgba.a * clamp(top_coverage, 0.0, 1.0);
    let bottom_alpha = bottom_rgba.a;
    let output_alpha = top_alpha + (bottom_alpha * (1.0 - top_alpha));
    if output_alpha <= 0.0 {
        return vec4<f32>(0.0);
    }
    let premultiplied_rgb =
        (top_rgba.rgb * top_alpha) +
        (bottom_rgba.rgb * bottom_alpha * (1.0 - top_alpha));
    return vec4<f32>(premultiplied_rgb / output_alpha, output_alpha);
}

fn border_color(local_pos_px: vec2<f32>, size_px: vec2<f32>, corner_radii_px: vec4<f32>, border_widths_px: vec4<f32>, top_rgba: vec4<f32>, right_rgba: vec4<f32>, bottom_rgba: vec4<f32>, left_rgba: vec4<f32>) -> vec4<f32> {
    let x = local_pos_px.x;
    let y = local_pos_px.y;
    let right_distance_px = size_px.x - x;
    let bottom_distance_px = size_px.y - y;

    let top_is_nearer_than_bottom = y <= bottom_distance_px;
    let horizontal_distance_px = select(bottom_distance_px, y, top_is_nearer_than_bottom);
    let horizontal_border_width_px = select(
        border_widths_px.z,
        border_widths_px.x,
        top_is_nearer_than_bottom
    );
    let horizontal_rgba = select(bottom_rgba, top_rgba, top_is_nearer_than_bottom);

    let left_is_nearer_than_right = x <= right_distance_px;
    let vertical_distance_px = select(right_distance_px, x, left_is_nearer_than_right);
    let vertical_border_width_px = select(
        border_widths_px.y,
        border_widths_px.w,
        left_is_nearer_than_right
    );
    let vertical_rgba = select(right_rgba, left_rgba, left_is_nearer_than_right);

    if horizontal_border_width_px <= 0.0 && vertical_border_width_px <= 0.0 {
        return vec4<f32>(0.0);
    }

    let corner_radius_px = corner_radius_for_edges(
        top_is_nearer_than_bottom,
        left_is_nearer_than_right,
        clamped_corner_radii(size_px, corner_radii_px)
    );
    let in_corner_curve =
        corner_radius_px > 0.0 &&
        horizontal_distance_px <= corner_radius_px &&
        vertical_distance_px <= corner_radius_px;
    if in_corner_curve {
        if horizontal_border_width_px <= 0.0 {
            return vertical_rgba;
        }
        if vertical_border_width_px <= 0.0 {
            return horizontal_rgba;
        }
        return corner_blend(
            horizontal_rgba,
            vertical_rgba,
            vertical_distance_px,
            horizontal_distance_px
        );
    }

    if horizontal_border_width_px > 0.0 && horizontal_distance_px <= horizontal_border_width_px {
        return horizontal_rgba;
    }
    if vertical_border_width_px > 0.0 && vertical_distance_px <= vertical_border_width_px {
        return vertical_rgba;
    }
    return vec4<f32>(0.0);
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let outer_alpha = rounded_rect_alpha(input.local_pos_px, input.size_px, input.corner_radii_px);
    if outer_alpha <= 0.0 {
        discard;
    }
    let inner_origin_px = vec2<f32>(input.border_widths_px.w, input.border_widths_px.x);
    let inner_size_px = max(
        input.size_px - vec2<f32>(
            input.border_widths_px.w + input.border_widths_px.y,
            input.border_widths_px.x + input.border_widths_px.z
        ),
        vec2<f32>(0.0, 0.0)
    );
    let inner_local_pos_px = input.local_pos_px - inner_origin_px;
    let inner_alpha = select(
        0.0,
        rounded_rect_alpha(
            inner_local_pos_px,
            inner_size_px,
            inner_corner_radii(input.corner_radii_px, input.border_widths_px)
        ),
        inner_size_px.x > 0.0 && inner_size_px.y > 0.0
    );
    let border_rgba = border_color(
        input.local_pos_px,
        input.size_px,
        input.corner_radii_px,
        input.border_widths_px,
        input.border_top_rgba,
        input.border_right_rgba,
        input.border_bottom_rgba,
        input.border_left_rgba
    );
    let base_rgba = rgba_over_with_coverage(
        input.fill_rgba,
        border_rgba,
        1.0 - clamp(inner_alpha, 0.0, 1.0)
    );
    return vec4<f32>(base_rgba.rgb, base_rgba.a * outer_alpha);
}
