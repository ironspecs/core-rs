//! Bevy base-material shader handle and shader asset registration for `ui-dom`.
//!
//! The `Material2d` type and its uniform layout live in `ui_dom_base_material.rs`; this file
//! owns only the shader handle that ties the material to its WGSL source and the loader that
//! registers it at app startup.
//!
//! Strict invariants:
//! - the shader loaded here must stay separate from the retained paint-family shaders under
//!   `domains/layer_pool/`.
//! - the shader handle is the single compile-time identity for the base material.

use ::bevy::{
    app::App,
    asset::{Handle, load_internal_asset, uuid_handle},
    shader::Shader,
};

pub(crate) use crate::bevy::ui_dom_base_material::UiDomBaseMaterial;

pub(crate) const UI_DOM_BASE_MATERIAL_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("d77ad8f7-c9ea-44e2-9f80-7435f15c2a5a");

pub(crate) fn load_base_material_shader(app: &mut App) {
    load_internal_asset!(
        app,
        UI_DOM_BASE_MATERIAL_SHADER_HANDLE,
        "shaders/base_material.wgsl",
        Shader::from_wgsl
    );
}
