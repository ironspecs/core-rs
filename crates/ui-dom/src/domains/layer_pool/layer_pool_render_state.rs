//! Render-owned surface state for the retained layer pool.
//!
//! This file owns the `LayerPoolRenderState` struct that holds the retained
//! rect, icon, text-run, svg, and image pipeline states for one render
//! executor, and the plain `RenderSurfaceApplyResult` returned after applying
//! a staged plan. Its purpose is to keep Render-world apply/install behavior
//! beside the Render-owned state instead of orchestrating it through free
//! functions in the shared `layer_pool` owner module.
//!
//! Invariants and rules:
//!
//! - this file owns the `LayerPoolRenderState` struct and its `impl` blocks,
//!   plus the plain `RenderSurfaceApplyResult` data type
//! - GPU pipeline and buffer creation stays in the shared `layer_pool` owner
//!   module free functions; this file calls them during construction and apply

use bevy::render::{
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
};

use crate::domains::render_ops::StagedLayerApply;
use crate::types::PixelDensity;

use super::layer_config::FrozenLayerConfig;
use super::layer_pool_text_render::{
    TextRunRenderState, TextRunSurfaceContext, create_text_run_render_state,
};
use super::{
    DIRECT_IMAGE_SHADER, DIRECT_SVG_SHADER, IconRenderState, LayerPipelineState,
    PendingIconAtlasInstall, apply_icons_to_surface, apply_images_to_surface,
    apply_rects_to_surface, apply_svgs_to_surface, create_debug_rect_pipeline_state,
    create_icon_render_state, create_rect_pipeline_state, install_ready_icon_atlas,
};

pub(super) struct RenderSurfaceApplyResult {
    pub(super) rect_buffer_grew: bool,
    pub(super) text_runs_ready: bool,
}

pub(super) struct LayerPoolRenderState {
    rect: LayerPipelineState,
    icon: IconRenderState,
    pub(super) text_run: TextRunRenderState,
    svg: LayerPipelineState,
    image: LayerPipelineState,
}

impl LayerPoolRenderState {
    pub(super) fn new(device: &RenderDevice, layer_config: FrozenLayerConfig) -> Self {
        Self {
            rect: create_rect_pipeline_state(device),
            icon: create_icon_render_state(device),
            text_run: create_text_run_render_state(device, layer_config.pixel_density()),
            svg: create_debug_rect_pipeline_state(device, "svg", DIRECT_SVG_SHADER),
            image: create_debug_rect_pipeline_state(device, "image", DIRECT_IMAGE_SHADER),
        }
    }
}

impl LayerPoolRenderState {
    pub(super) fn apply_plan_to_surface(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        gpu_image: &GpuImage,
        pixel_density: PixelDensity,
        apply: &StagedLayerApply,
    ) -> RenderSurfaceApplyResult {
        let rect_buffer_grew = apply_rects_to_surface(
            &mut self.rect,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.rects,
            apply.layer_zero_in_image_px,
        );
        apply_icons_to_surface(
            &mut self.icon,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.icons,
            apply.layer_zero_in_image_px,
        );
        let text_runs_ready = self.text_run.apply_to_surface(
            &TextRunSurfaceContext {
                device,
                queue,
                gpu_image,
                layer_zero_in_image_px: apply.layer_zero_in_image_px,
                lease: apply.lease,
            },
            &apply.plan.text_runs,
        );
        apply_svgs_to_surface(
            &mut self.svg,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.svgs,
            apply.layer_zero_in_image_px,
        );
        apply_images_to_surface(
            &mut self.image,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.images,
            apply.layer_zero_in_image_px,
        );
        RenderSurfaceApplyResult {
            rect_buffer_grew,
            text_runs_ready,
        }
    }

    pub(super) fn install_ready_icon_atlas(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        pending_install: PendingIconAtlasInstall,
    ) {
        install_ready_icon_atlas(device, queue, &mut self.icon, pending_install);
    }

    pub(super) fn icon_atlas_is_ready(&self) -> bool {
        self.icon.atlas.is_some()
    }
}
