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

#[cfg(test)]
use crate::domains::layer_ops::{LayerPaintPlan, RectPaintOp};
#[cfg(test)]
use crate::domains::{LayerConfig, LayerLease, LayerPool};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::asset::Handle;
    use bevy::image::Image;
    use bevy::prelude::World;
    use bevy::render::{
        render_resource::{Sampler, Texture},
        renderer::{RenderDevice, RenderQueue, WgpuWrapper},
        texture::GpuImage,
    };
    use bevy::tasks::block_on;

    use super::*;

    fn noop_render_device_and_queue() -> (RenderDevice, RenderQueue) {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::NOOP,
            backend_options: wgpu::BackendOptions {
                noop: wgpu::NoopBackendOptions { enable: true },
                ..Default::default()
            },
            ..Default::default()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("noop adapter should exist");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("noop device should be granted");
        let render_device = RenderDevice::from(device);
        let render_queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
        (render_device, render_queue)
    }

    fn noop_gpu_image(render_device: &RenderDevice) -> GpuImage {
        let size = wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        };
        let texture = render_device
            .wgpu_device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("ui_dom_test_noop_surface"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
        let texture = Texture::from(texture);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = Sampler::from(
            render_device
                .wgpu_device()
                .create_sampler(&wgpu::SamplerDescriptor::default()),
        );
        GpuImage {
            texture,
            texture_view,
            texture_format: wgpu::TextureFormat::Rgba8Unorm,
            texture_view_format: None,
            sampler,
            size,
            mip_level_count: 1,
            had_data: false,
        }
    }

    fn single_rect_op() -> RectPaintOp {
        RectPaintOp {
            position_px: [0.0, 0.0],
            size_px: [8.0, 8.0],
            fill_rgba: [1.0, 1.0, 1.0, 1.0],
            corner_radii_px: [0.0; 4],
            border_widths_px: [0.0; 4],
            border_top_rgba: [0.0; 4],
            border_right_rgba: [0.0; 4],
            border_bottom_rgba: [0.0; 4],
            border_left_rgba: [0.0; 4],
        }
    }

    fn lease_in_world() -> LayerLease {
        let mut world = World::new();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 10.0, 10.0))
    }

    fn staged_apply(lease: LayerLease, plan: LayerPaintPlan) -> StagedLayerApply {
        StagedLayerApply {
            lease,
            image_handle: Handle::<Image>::default(),
            layer_zero_in_image_px: [0.0, 0.0],
            plan,
        }
    }

    #[test]
    fn render_state_constructs_all_five_pipelines() {
        let (render_device, _render_queue) = noop_render_device_and_queue();
        let state = LayerPoolRenderState::new(
            &render_device,
            FrozenLayerConfig::new(LayerConfig::new(PixelDensity::X1)),
        );
        assert!(!state.icon_atlas_is_ready());
    }

    #[test]
    fn apply_plan_with_one_rect_repaints_into_noop_surface() {
        let (render_device, render_queue) = noop_render_device_and_queue();
        let mut state = LayerPoolRenderState::new(
            &render_device,
            FrozenLayerConfig::new(LayerConfig::new(PixelDensity::X1)),
        );
        let gpu_image = noop_gpu_image(&render_device);
        let lease = lease_in_world();

        let single_rect_apply = staged_apply(
            lease,
            LayerPaintPlan {
                rects: vec![single_rect_op()],
                ..LayerPaintPlan::default()
            },
        );
        let result = state.apply_plan_to_surface(
            &render_device,
            &render_queue,
            &gpu_image,
            PixelDensity::X1,
            &single_rect_apply,
        );
        assert!(result.text_runs_ready);
        assert!(
            !result.rect_buffer_grew,
            "the rect instance buffer starts at capacity 1, so one rect does not grow it"
        );

        let two_rect_apply = staged_apply(
            lease,
            LayerPaintPlan {
                rects: vec![single_rect_op(), single_rect_op()],
                ..LayerPaintPlan::default()
            },
        );
        let result = state.apply_plan_to_surface(
            &render_device,
            &render_queue,
            &gpu_image,
            PixelDensity::X1,
            &two_rect_apply,
        );
        assert!(result.rect_buffer_grew);
    }
}
