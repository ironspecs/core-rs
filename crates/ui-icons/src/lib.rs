//! Public crate surface for `ui-icons`.
//!
//! This file owns the stable icon constants plus atlas path helpers shared by
//! UI crates and asset-generation tools; the icon identifier types live in
//! `ui_icon_density.rs` and `ui_icon_id.rs` and are re-exported here. The
//! surface must stay free of app-domain types so the icon contract can move
//! independently of any one runtime.

#![forbid(unsafe_code)]

mod ui_icon_density;
mod ui_icon_id;

pub use ui_icon_density::UiIconDensity;
pub use ui_icon_id::UiIconId;

/// Relative asset directory for generated retained UI icon atlas artifacts.
pub const UI_ICON_GENERATED_ASSET_DIR: &str = "icons/generated";
/// Stable square design-box size, in logical pixels, used during atlas generation.
pub const UI_ICON_DESIGN_BOX_PX: u32 = 24;

/// Stable number of curated flat UI icons.
pub const UI_ICON_COUNT: usize = UiIconId::ALL.len();

/// Returns the generated PNG asset path for one retained icon density.
#[must_use]
pub const fn ui_icon_atlas_png_asset_path(density: UiIconDensity) -> &'static str {
    match density {
        UiIconDensity::X1 => "icons/generated/ui-icons-x1.png",
        UiIconDensity::X2 => "icons/generated/ui-icons-x2.png",
        UiIconDensity::X4 => "icons/generated/ui-icons-x4.png",
    }
}

/// Returns the generated JSON manifest asset path for one retained icon density.
#[must_use]
pub const fn ui_icon_atlas_manifest_asset_path(density: UiIconDensity) -> &'static str {
    match density {
        UiIconDensity::X1 => "icons/generated/ui-icons-x1.json",
        UiIconDensity::X2 => "icons/generated/ui-icons-x2.json",
        UiIconDensity::X4 => "icons/generated/ui-icons-x4.json",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        UI_ICON_COUNT, UI_ICON_DESIGN_BOX_PX, UiIconDensity, UiIconId,
        ui_icon_atlas_manifest_asset_path, ui_icon_atlas_png_asset_path,
    };

    #[test]
    fn icon_count_matches_curated_set() {
        assert_eq!(UiIconId::ALL.len(), UI_ICON_COUNT);
    }

    #[test]
    fn density_artifacts_have_stable_paths_and_design_box_sizes() {
        assert_eq!(UiIconDensity::ALL.len(), 3);
        assert_eq!(
            ui_icon_atlas_png_asset_path(UiIconDensity::X1),
            "icons/generated/ui-icons-x1.png"
        );
        assert_eq!(
            ui_icon_atlas_manifest_asset_path(UiIconDensity::X4),
            "icons/generated/ui-icons-x4.json"
        );
        assert_eq!(
            UiIconDensity::X1.design_box_size_px(),
            UI_ICON_DESIGN_BOX_PX
        );
        assert_eq!(
            UiIconDensity::X2.design_box_size_px(),
            UI_ICON_DESIGN_BOX_PX * 2
        );
        assert_eq!(
            UiIconDensity::X4.design_box_size_px(),
            UI_ICON_DESIGN_BOX_PX * 4
        );
    }
}
