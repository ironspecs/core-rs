//! `OrderedTransparentPassNode` runs the `ui-dom` ordered transparent render pass per view.
//! This file owns the Bevy render-graph view node that presents retained surfaces in z order.
//!
//! Queueing, sorting, and item preparation stay in `ordered_surface_render.rs`; this node only
//! walks the queued ordered items through the retained transparent draw functions.
//!
//! Strict invariants:
//! - this node renders already-ordered items; it does not sort or queue.
//! - the pass never allocates on the hot path.

use bevy::{
    prelude::World,
    render::{
        camera::ExtractedCamera,
        diagnostic::RecordDiagnostics,
        render_graph::{NodeRunError, RenderGraphContext, ViewNode},
        render_phase::TrackedRenderPass,
        render_resource::{CommandEncoderDescriptor, RenderPassDescriptor, StoreOp},
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};

use crate::bevy::ordered_surface_render::render_ordered_transparent_items;
use crate::bevy::ordered_transparent_ui_dom_surfaces_by_view::OrderedTransparentUiDomSurfacesByView;

#[derive(Default)]
pub(crate) struct OrderedTransparentPassNode;

impl ViewNode for OrderedTransparentPassNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static ViewDepthTexture,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut bevy::render::renderer::RenderContext<'w>,
        (camera, view, target, depth): bevy::ecs::query::QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let Some(ordered_items_by_view) =
            world.get_resource::<OrderedTransparentUiDomSurfacesByView>()
        else {
            return Ok(());
        };
        let Some(ordered_items) = ordered_items_by_view.items_for_view(&view.retained_view_entity)
        else {
            return Ok(());
        };
        if ordered_items.is_empty() {
            return Ok(());
        }

        let view_entity = graph.view_entity();
        let diagnostics = render_context.diagnostic_recorder();
        let color_attachments = [Some(target.get_color_attachment())];
        let depth_stencil_attachment = Some(depth.get_attachment(StoreOp::Store));

        render_context.add_command_buffer_generation_task(move |render_device| {
            let mut command_encoder =
                render_device.create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("ordered_transparent_pass_command_encoder"),
                });

            {
                let render_pass = command_encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("ordered_transparent_pass"),
                    color_attachments: &color_attachments,
                    depth_stencil_attachment,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                let mut render_pass = TrackedRenderPass::new(&render_device, render_pass);
                let pass_span = diagnostics.pass_span(&mut render_pass, "ordered_transparent_pass");

                if let Some(viewport) = camera.viewport.as_ref() {
                    render_pass.set_camera_viewport(viewport);
                }

                if let Err(error) = render_ordered_transparent_items(
                    ordered_items,
                    &mut render_pass,
                    world,
                    view_entity,
                ) {
                    tracing::error!(
                        "Error encountered while rendering ordered transparent items {error:?}"
                    );
                }

                pass_span.end(&mut render_pass);
            }

            command_encoder.finish()
        });

        Ok(())
    }
}
