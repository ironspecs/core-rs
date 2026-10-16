//! Icon-atlas schema validation and runtime atlas contract for `ui-dom`.
//!
//! This file exists so icon-atlas density mapping, manifest validation, and
//! runtime sampling geometry stay with the `ui-dom` domain contract instead of
//! hiding inside Bevy asset-loading glue.
//!
//! Strict invariants:
//! - this file validates committed atlas contents; it does not request or load
//!   Bevy assets
//! - packed icon interiors must match the retained design-box size exactly
//! - runtime atlas lookup stays keyed by `UiIconId` and retained density

use std::sync::Arc;

use anyhow::{Context, anyhow, ensure};
use bevy::image::Image;
use serde::Deserialize;
use ui_icons::{UiIconDensity, UiIconId};

use crate::bevy::icon_assets::UiIconAtlasManifestAsset;
use crate::types::PixelDensity;

#[derive(Clone)]
pub(crate) struct ValidatedUiIconAtlasManifest {
    pub(crate) atlas_width_px: u32,
    pub(crate) atlas_height_px: u32,
    pub(crate) design_box_size_px: u32,
    pub(crate) lookup: ValidatedUiIconAtlasLookup,
}

#[derive(Clone)]
pub(crate) struct ValidatedUiIconAtlasLookup {
    entries: Arc<[ValidatedUiIconAtlasEntry]>,
}

#[derive(Clone, Copy)]
pub(crate) struct ValidatedUiIconAtlasEntry {
    pub(crate) packed_rect_px: [u32; 4],
}

#[derive(Deserialize)]
struct RawUiIconAtlasManifest {
    retained_density_name: String,
    retained_density_scale: u32,
    design_box_size_px: u32,
    atlas_width_px: u32,
    atlas_height_px: u32,
    icons: Vec<RawUiIconAtlasIconRecord>,
}

#[derive(Deserialize)]
struct RawUiIconAtlasIconRecord {
    ui_icon_id: u16,
    debug_stem: Option<String>,
    packed_rect_px: RawUiIconRect,
}

#[derive(Deserialize)]
#[allow(clippy::struct_field_names)]
struct RawUiIconRect {
    min_x_px: u32,
    min_y_px: u32,
    width_px: u32,
    height_px: u32,
}

impl ValidatedUiIconAtlasLookup {
    #[must_use]
    pub(crate) fn entry(&self, icon_id: UiIconId) -> &ValidatedUiIconAtlasEntry {
        &self.entries[icon_id.dense_index()]
    }
}

#[cfg(test)]
impl ValidatedUiIconAtlasLookup {
    #[must_use]
    pub(crate) fn from_entries_for_tests(entries: Vec<ValidatedUiIconAtlasEntry>) -> Self {
        Self {
            entries: Arc::<[ValidatedUiIconAtlasEntry]>::from(entries),
        }
    }
}

pub(crate) fn icon_density_from_pixel_density(pixel_density: PixelDensity) -> UiIconDensity {
    match pixel_density {
        PixelDensity::X1 => UiIconDensity::X1,
        PixelDensity::X2 => UiIconDensity::X2,
        PixelDensity::X4 => UiIconDensity::X4,
    }
}

pub(crate) fn validate_manifest_asset(
    manifest_asset: &UiIconAtlasManifestAsset,
    expected_density: UiIconDensity,
) -> anyhow::Result<ValidatedUiIconAtlasManifest> {
    validate_ui_icon_atlas_manifest(&manifest_asset.json, expected_density)
}

fn validate_ui_icon_atlas_manifest(
    manifest_json: &str,
    expected_density: UiIconDensity,
) -> anyhow::Result<ValidatedUiIconAtlasManifest> {
    let raw: RawUiIconAtlasManifest =
        serde_json::from_str(manifest_json).context("parse ui icon atlas manifest json")?;
    validate_manifest_density(&raw, expected_density)?;
    let mut entries = vec![None; UiIconId::ALL.len()];
    for raw_icon in raw.icons {
        let (icon_id, entry) = validated_icon_entry_from_raw(
            &raw_icon,
            raw.atlas_width_px,
            raw.atlas_height_px,
            raw.design_box_size_px,
        )?;
        ensure!(
            entries[icon_id.dense_index()].is_none(),
            "ui icon atlas manifest should cover each UiIconId exactly once"
        );
        entries[icon_id.dense_index()] = Some(entry);
    }
    let dense_entries = entries
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow!("ui icon atlas manifest should cover UiIconId::ALL exactly once"))?;
    Ok(ValidatedUiIconAtlasManifest {
        atlas_width_px: raw.atlas_width_px,
        atlas_height_px: raw.atlas_height_px,
        design_box_size_px: raw.design_box_size_px,
        lookup: ValidatedUiIconAtlasLookup {
            entries: Arc::<[ValidatedUiIconAtlasEntry]>::from(dense_entries),
        },
    })
}

fn validate_manifest_density(
    raw: &RawUiIconAtlasManifest,
    expected_density: UiIconDensity,
) -> anyhow::Result<()> {
    ensure!(
        raw.retained_density_name == expected_density.file_suffix(),
        "ui icon atlas manifest density name should match the expected retained density"
    );
    ensure!(
        raw.retained_density_scale == expected_density.multiplier(),
        "ui icon atlas manifest density scale should match the expected retained density"
    );
    ensure!(
        raw.design_box_size_px == expected_density.design_box_size_px(),
        "ui icon atlas manifest design box size should match the expected retained density"
    );
    Ok(())
}

fn validated_icon_entry_from_raw(
    raw_icon: &RawUiIconAtlasIconRecord,
    atlas_width_px: u32,
    atlas_height_px: u32,
    design_box_size_px: u32,
) -> anyhow::Result<(UiIconId, ValidatedUiIconAtlasEntry)> {
    let icon_id = UiIconId::from_numeric_id(raw_icon.ui_icon_id).ok_or_else(|| {
        anyhow!(
            "ui icon atlas manifest contains an unknown UiIconId: {}",
            raw_icon.ui_icon_id
        )
    })?;
    if let Some(debug_stem) = raw_icon.debug_stem.as_deref() {
        ensure!(
            debug_stem == icon_id.file_stem(),
            "ui icon atlas manifest stem should match UiIconId ordering"
        );
    }
    validate_packed_rect(raw_icon, atlas_width_px, atlas_height_px)?;
    validate_packed_extent_matches_design_box(raw_icon, design_box_size_px)?;
    Ok((
        icon_id,
        ValidatedUiIconAtlasEntry {
            packed_rect_px: [
                raw_icon.packed_rect_px.min_x_px,
                raw_icon.packed_rect_px.min_y_px,
                raw_icon.packed_rect_px.width_px,
                raw_icon.packed_rect_px.height_px,
            ],
        },
    ))
}

fn validate_packed_extent_matches_design_box(
    raw_icon: &RawUiIconAtlasIconRecord,
    design_box_size_px: u32,
) -> anyhow::Result<()> {
    ensure!(
        raw_icon.packed_rect_px.width_px == design_box_size_px,
        "icon atlas packed width should match the design box width"
    );
    ensure!(
        raw_icon.packed_rect_px.height_px == design_box_size_px,
        "icon atlas packed height should match the design box height"
    );
    Ok(())
}

fn validate_packed_rect(
    raw_icon: &RawUiIconAtlasIconRecord,
    atlas_width_px: u32,
    atlas_height_px: u32,
) -> anyhow::Result<()> {
    ensure!(
        raw_icon.packed_rect_px.width_px > 0 && raw_icon.packed_rect_px.height_px > 0,
        "icon atlas packed rect should have a non-zero extent"
    );
    ensure!(
        raw_icon.packed_rect_px.min_x_px + raw_icon.packed_rect_px.width_px <= atlas_width_px,
        "icon atlas packed rect should stay inside atlas width"
    );
    ensure!(
        raw_icon.packed_rect_px.min_y_px + raw_icon.packed_rect_px.height_px <= atlas_height_px,
        "icon atlas packed rect should stay inside atlas height"
    );
    Ok(())
}

pub(crate) fn alpha_mask_bytes_from_image(image: &Image) -> anyhow::Result<Arc<[u8]>> {
    let rgba_bytes = image
        .data
        .as_ref()
        .ok_or_else(|| anyhow!("ui icon atlas image should keep cpu-side bytes"))?;
    ensure!(
        rgba_bytes.len()
            == usize::try_from(image.texture_descriptor.size.width)
                .expect("image width should fit usize")
                * usize::try_from(image.texture_descriptor.size.height)
                    .expect("image height should fit usize")
                * 4,
        "ui icon atlas image should keep tightly packed RGBA8 bytes"
    );
    let alpha_mask_bytes = rgba_bytes
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    Ok(Arc::<[u8]>::from(alpha_mask_bytes))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ui_icons::{UiIconDensity, ui_icon_atlas_manifest_asset_path};

    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        image::Image,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("ui-dom manifest dir should stay under crates/<name> inside the workspace root")
            .to_path_buf()
    }

    #[test]
    fn pixel_density_maps_to_ui_icon_density() {
        assert_eq!(
            icon_density_from_pixel_density(PixelDensity::X1),
            UiIconDensity::X1
        );
        assert_eq!(
            icon_density_from_pixel_density(PixelDensity::X4),
            UiIconDensity::X4
        );
    }

    #[test]
    fn generated_manifests_cover_curated_icon_ids_for_all_densities() {
        for density in UiIconDensity::ALL {
            let manifest_path = workspace_root()
                .join("assets")
                .join(ui_icon_atlas_manifest_asset_path(density));
            let json = std::fs::read_to_string(manifest_path).expect("manifest");
            let validated = validate_ui_icon_atlas_manifest(&json, density).expect("valid");

            assert_eq!(
                validated.lookup.entries.len(),
                ui_icons::UiIconId::ALL.len()
            );
            assert_eq!(validated.design_box_size_px, density.design_box_size_px());
        }
    }

    #[test]
    fn generated_manifests_pack_full_design_box_interiors_for_all_densities() {
        for density in UiIconDensity::ALL {
            let manifest_path = workspace_root()
                .join("assets")
                .join(ui_icon_atlas_manifest_asset_path(density));
            let json = std::fs::read_to_string(manifest_path).expect("manifest");
            let validated = validate_ui_icon_atlas_manifest(&json, density).expect("valid");

            for entry in &*validated.lookup.entries {
                assert_eq!(entry.packed_rect_px[2], validated.design_box_size_px);
                assert_eq!(entry.packed_rect_px[3], validated.design_box_size_px);
            }
        }
    }

    #[test]
    fn alpha_mask_bytes_extract_the_alpha_channel_only() {
        let image = Image::new_fill(
            Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[10, 20, 30, 40, 50, 60, 70, 80],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );

        let alpha = alpha_mask_bytes_from_image(&image).expect("alpha");

        assert_eq!(&*alpha, &[40, 80]);
    }
}
