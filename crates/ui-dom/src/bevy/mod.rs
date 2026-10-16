//! Bevy-specific integration boundary for `ui-dom`.
//!
//! This folder exists for Bevy-only asset/material setup and other pure Bevy
//! glue that should not live in the `ui-dom` domain owners.
//!
//! Keep only Bevy-only adapters here: Bevy presentation material boilerplate,
//! shader asset loading, and raw Bevy asset-loader seams such as font and icon
//! atlas manifest loading.
//! If a type would still exist without Bevy, it probably belongs in
//! `domains/` or another non-Bevy crate boundary instead.
//!
//! Strict invariants:
//! - this folder is for Bevy host glue, not `ui-dom` domain ownership.
//! - domain runtime owners such as `LayerPool` stay outside this boundary.
//! - mixed install/runtime wiring stays outside this boundary.
//! - shaders here are Bevy presentation shaders, not retained layer paint
//!   shaders.

pub(crate) mod base_material;
pub(crate) mod font_assets;
pub(crate) mod icon_assets;
pub(crate) mod ordered_surface_render;
pub(crate) mod ordered_transparent_pass_node;
pub(crate) mod ordered_transparent_ui_dom_surfaces_by_view;
pub(crate) mod ui_dom_base_material;
pub(crate) mod ui_dom_font_asset_loader;
pub(crate) mod ui_icon_atlas_manifest_asset_loader;
