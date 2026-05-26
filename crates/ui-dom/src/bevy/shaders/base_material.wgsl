// Bevy presentation shader for already-rendered `ui-dom` layer textures.
// This shader exists to clip and sample the retained layer image on the host
// quad; it is not part of the retained paint-family execution boundary.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct UiDomBaseMaterial {
    color: vec4<f32>,
    clip_world: vec4<f32>,
    uv_window: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: UiDomBaseMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var color_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var color_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let world = mesh.world_position.xy;
    if world.x < material.clip_world.x
        || world.y < material.clip_world.y
        || world.x > material.clip_world.z
        || world.y > material.clip_world.w {
        discard;
    }
    let uv = vec2<f32>(
        mix(material.uv_window.x, material.uv_window.z, mesh.uv.x),
        mix(material.uv_window.y, material.uv_window.w, mesh.uv.y),
    );
    return material.color * textureSample(color_texture, color_sampler, uv);
}
