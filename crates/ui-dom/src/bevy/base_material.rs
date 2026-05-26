//! Bevy `Material2d` base material for `ui-dom` retained layer textures.
//!
//! This file exists because Bevy needs a material type, shader handle, and
//! shader asset registration to present retained layer images, but that is host
//! boilerplate rather than domain logic.
//!
//! This boundary is only for presenting already-rendered layer textures on Bevy
//! quads. It does not own retained layer painting, retained surface lifetime,
//! or icon/text raster policy.
//!
//! Strict invariants:
//! - this material presents retained layer textures; it does not paint into
//!   them.
//! - clipping and UV windowing here are host presentation concerns only.
//! - the shader loaded here must stay separate from the retained paint-family
//!   shaders under `domains/layer_pool/`.

use ::bevy::{
    app::App,
    asset::{Handle, load_internal_asset, uuid_handle},
    image::Image,
    math::Vec4,
    prelude::Asset,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::{Shader, ShaderRef},
    sprite_render::{AlphaMode2d, Material2d},
};

const UI_DOM_BASE_MATERIAL_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("d77ad8f7-c9ea-44e2-9f80-7435f15c2a5a");

/// Bevy base presentation material for one retained `ui-dom` layer texture.
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub(crate) struct UiDomBaseMaterial {
    #[uniform(0)]
    pub uniform: UiDomBaseMaterialUniform,
    #[texture(1)]
    #[sampler(2)]
    pub color_texture: Handle<Image>,
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub(crate) struct UiDomBaseMaterialUniform {
    pub color: Vec4,
    pub clip_world: Vec4,
    pub uv_window: Vec4,
}

impl UiDomBaseMaterial {
    pub(crate) fn new(color_texture: Handle<Image>, opacity: f32) -> Self {
        Self {
            uniform: UiDomBaseMaterialUniform {
                color: Vec4::new(1.0, 1.0, 1.0, opacity),
                clip_world: Vec4::new(-1_000_000.0, -1_000_000.0, 1_000_000.0, 1_000_000.0),
                uv_window: Vec4::new(0.0, 0.0, 1.0, 1.0),
            },
            color_texture,
        }
    }
}

impl Material2d for UiDomBaseMaterial {
    fn fragment_shader() -> ShaderRef {
        UI_DOM_BASE_MATERIAL_SHADER_HANDLE.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

pub(crate) fn load_base_material_shader(app: &mut App) {
    load_internal_asset!(
        app,
        UI_DOM_BASE_MATERIAL_SHADER_HANDLE,
        "shaders/base_material.wgsl",
        Shader::from_wgsl
    );
}
