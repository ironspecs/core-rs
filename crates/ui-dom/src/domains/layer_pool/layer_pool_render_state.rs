//! Render-state behavior for the retained layer pool.
//!
//! This file exists so Render-world apply/install behavior lives beside the
//! Render-owned state instead of being orchestrated through free functions in
//! the shared `layer_pool` owner module.

use super::layer_pool_text_render::TextRunSurfaceContext;
#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) struct RenderSurfaceApplyResult {
    pub(super) rect_buffer_grew: bool,
    pub(super) text_runs_ready: bool,
}

impl LayerPoolRenderState {
    pub(super) fn apply_plan_to_surface(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        gpu_image: &GpuImage,
        pixel_density: PixelDensity,
        apply: &PendingLayerApply,
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

impl IconAtlasLoadState {
    pub(super) fn take_pending_install(&mut self) -> Option<PendingIconAtlasInstall> {
        let IconAtlasLoadState::Loading(loading_state) = self else {
            return None;
        };
        loading_state.pending_install.take()
    }
}
