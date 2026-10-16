//! `UiDomBaseMaterial` presents one retained `ui-dom` layer texture through a Bevy `Material2d`.
//! This file owns the material type, its GPU uniform layout, and the shader-bind plumbing.
//!
//! The shader handle and shader registration stay in `base_material.rs`; this file only declares
//! the material so Bevy can bind retained layer images to a quad.
//!
//! Strict invariants:
//! - this material presents retained layer textures; it does not paint into them.
//! - clipping and UV windowing here are host presentation concerns only.

use ::bevy::{
    asset::Handle,
    image::Image,
    math::Vec4,
    prelude::Asset,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d},
};

use crate::bevy::base_material::UI_DOM_BASE_MATERIAL_SHADER_HANDLE;

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
