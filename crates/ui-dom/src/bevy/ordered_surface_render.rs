use std::collections::{HashMap, HashSet};

use bevy::{
    app::SubApp,
    core_pipeline::core_2d::{AlphaMask2d, Opaque2d, Transparent2d},
    core_pipeline::core_2d::{graph::Core2d, graph::Node2d},
    ecs::system::StaticSystemParam,
    math::FloatOrd,
    prelude::{Entity, IntoScheduleConfigs, Local, Mesh2d, Query, Res, ResMut, Resource, World},
    render::{
        ExtractSchedule, Render, RenderStartup, RenderSystems,
        batching::{GetBatchData, no_gpu_preprocessing::BatchedInstanceBuffer},
        camera::{ExtractedCamera, extract_cameras},
        diagnostic::RecordDiagnostics,
        mesh::RenderMesh,
        render_asset::{RenderAssets, prepare_assets},
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_phase::{
            AddRenderCommand, DrawError, DrawFunctions, PhaseItem, PhaseItemExtraIndex,
            SetItemPipeline, TrackedRenderPass,
        },
        render_resource::{
            CachedRenderPipelineId, CommandEncoderDescriptor, RenderPassDescriptor,
            SpecializedMeshPipelines, StoreOp,
        },
        sync_world::MainEntity,
        view::{
            ExtractedView, RenderVisibleEntities, RetainedViewEntity, ViewDepthTexture, ViewTarget,
        },
    },
    sprite_render::{
        DrawMesh2d, EntitySpecializationTickPair, Material2dPipeline, Mesh2dPipeline,
        Mesh2dUniform, PreparedMaterial2d, RenderMaterial2dInstances, RenderMesh2dInstances,
        SetMaterial2dBindGroup, SetMesh2dBindGroup, SetMesh2dViewBindGroup,
        SpecializedMaterial2dPipelineCache, extract_entities_needs_specialization,
        extract_mesh_materials_2d, init_material_2d_pipeline, init_mesh_2d_pipeline,
        specialize_material2d_meshes,
    },
};

use crate::{domains::LayerPoolRenderExecutor, render_plugin::UiDomBaseMaterial};

#[cfg(test)]
use crate::domains::LayerConfig;

type DrawUiDomSurfaceMaterial = (
    SetItemPipeline,
    SetMesh2dViewBindGroup<0>,
    SetMesh2dBindGroup<1>,
    SetMaterial2dBindGroup<UiDomBaseMaterial, 2>,
    DrawMesh2d,
);

#[derive(Default, Resource)]
struct OrderedTransparentUiDomSurfacesByView {
    items_by_view: HashMap<RetainedViewEntity, Vec<Transparent2d>>,
}

impl OrderedTransparentUiDomSurfacesByView {
    fn clear_live_views(&mut self, live_views: &HashSet<RetainedViewEntity>) {
        self.items_by_view.retain(|view, items| {
            let should_keep = live_views.contains(view);
            if should_keep {
                items.clear();
            }
            should_keep
        });
    }

    fn items_for_view_mut(&mut self, view: RetainedViewEntity) -> &mut Vec<Transparent2d> {
        self.items_by_view.entry(view).or_default()
    }

    fn items_for_view(&self, view: &RetainedViewEntity) -> Option<&[Transparent2d]> {
        self.items_by_view.get(view).map(Vec::as_slice)
    }
}

#[derive(Default)]
struct VisibleUiDomSurfaceScratch {
    visible_surface_entities_by_main_entity: HashMap<MainEntity, Entity>,
    visible_attached_root_parents: Vec<MainEntity>,
    seen_attached_root_parents: HashSet<MainEntity>,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct OrderedTransparentPassLabel;

#[derive(Default)]
struct OrderedTransparentPassNode;

pub(crate) fn install_ui_dom_ordered_surface_render_runtime(render_app: &mut SubApp) {
    render_app
        .init_resource::<EntitySpecializationTickPair<UiDomBaseMaterial>>()
        .init_resource::<OrderedTransparentUiDomSurfacesByView>()
        .init_resource::<RenderMaterial2dInstances<UiDomBaseMaterial>>()
        .init_resource::<SpecializedMaterial2dPipelineCache<UiDomBaseMaterial>>()
        .init_resource::<SpecializedMeshPipelines<Material2dPipeline<UiDomBaseMaterial>>>()
        .add_render_command::<Opaque2d, DrawUiDomSurfaceMaterial>()
        .add_render_command::<AlphaMask2d, DrawUiDomSurfaceMaterial>()
        .add_render_command::<Transparent2d, DrawUiDomSurfaceMaterial>()
        .add_systems(
            RenderStartup,
            init_material_2d_pipeline::<UiDomBaseMaterial>.after(init_mesh_2d_pipeline),
        )
        .add_systems(
            ExtractSchedule,
            (
                extract_entities_needs_specialization::<UiDomBaseMaterial>.after(extract_cameras),
                extract_mesh_materials_2d::<UiDomBaseMaterial>,
            ),
        )
        .add_systems(
            Render,
            (
                specialize_material2d_meshes::<UiDomBaseMaterial>
                    .in_set(RenderSystems::PrepareMeshes)
                    .after(prepare_assets::<PreparedMaterial2d<UiDomBaseMaterial>>)
                    .after(prepare_assets::<RenderMesh>),
                queue_ui_dom_ordered_surfaces
                    .in_set(RenderSystems::QueueMeshes)
                    .after(specialize_material2d_meshes::<UiDomBaseMaterial>),
                prepare_ui_dom_ordered_surfaces
                    .in_set(RenderSystems::PrepareResources)
                    .after(queue_ui_dom_ordered_surfaces),
            ),
        )
        .add_render_graph_node::<ViewNodeRunner<OrderedTransparentPassNode>>(
            Core2d,
            OrderedTransparentPassLabel,
        )
        .add_render_graph_edges(
            Core2d,
            (
                Node2d::MainTransparentPass,
                OrderedTransparentPassLabel,
                Node2d::EndMainPass,
            ),
        );
}

#[allow(clippy::needless_pass_by_value)]
fn queue_ui_dom_ordered_surfaces(
    (render_meshes, render_materials): (
        Res<RenderAssets<RenderMesh>>,
        Res<RenderAssets<PreparedMaterial2d<UiDomBaseMaterial>>>,
    ),
    (mut render_mesh_instances, mut ordered_items_by_view): (
        ResMut<RenderMesh2dInstances>,
        ResMut<OrderedTransparentUiDomSurfacesByView>,
    ),
    (render_material_instances, specialized_material_pipeline_cache, executor): (
        Res<RenderMaterial2dInstances<UiDomBaseMaterial>>,
        Res<SpecializedMaterial2dPipelineCache<UiDomBaseMaterial>>,
        Res<LayerPoolRenderExecutor>,
    ),
    views: Query<(&MainEntity, &ExtractedView, &RenderVisibleEntities)>,
    mut live_views: Local<HashSet<RetainedViewEntity>>,
    mut scratch: Local<VisibleUiDomSurfaceScratch>,
) {
    live_views.clear();
    for (_, view, _) in &views {
        live_views.insert(view.retained_view_entity);
    }
    ordered_items_by_view.clear_live_views(&live_views);
    if !executor.has_root_surface_orders() || render_material_instances.is_empty() {
        return;
    }

    for (view_main_entity, view, visible_entities) in &views {
        let Some(view_pipeline_cache) = specialized_material_pipeline_cache.get(view_main_entity)
        else {
            continue;
        };
        collect_visible_ui_dom_surfaces_and_attached_root_parents(
            visible_entities,
            &executor,
            &mut scratch,
        );
        sort_attached_root_parents_by_z_order(
            &executor,
            &mut scratch.visible_attached_root_parents,
        );
        let ordered_items = ordered_items_by_view.items_for_view_mut(view.retained_view_entity);

        for attached_root_parent in scratch.visible_attached_root_parents.iter().copied() {
            let Some(ordered_surface_entities) =
                executor.ordered_surface_entities_for_attached_root_parent(attached_root_parent)
            else {
                continue;
            };
            for main_entity in ordered_surface_entities.iter().copied() {
                let Some(render_entity) = scratch
                    .visible_surface_entities_by_main_entity
                    .get(&main_entity)
                    .copied()
                else {
                    continue;
                };
                let Some((_, pipeline_id)) = view_pipeline_cache.get(&main_entity).copied() else {
                    continue;
                };
                let Some(ordered_item) = prepare_ordered_transparent_item(
                    &render_meshes,
                    &render_materials,
                    &mut render_mesh_instances,
                    &render_material_instances,
                    main_entity,
                    render_entity,
                    pipeline_id,
                ) else {
                    continue;
                };
                ordered_items.push(ordered_item);
            }
        }
    }
}

fn collect_visible_ui_dom_surfaces_and_attached_root_parents(
    visible_entities: &RenderVisibleEntities,
    executor: &LayerPoolRenderExecutor,
    scratch: &mut VisibleUiDomSurfaceScratch,
) {
    scratch.visible_surface_entities_by_main_entity.clear();
    scratch.visible_attached_root_parents.clear();
    scratch.seen_attached_root_parents.clear();
    for (render_entity, visible_main_entity) in visible_entities.iter::<Mesh2d>() {
        let Some(attached_root_parent) =
            executor.attached_root_parent_for_surface_entity(*visible_main_entity)
        else {
            continue;
        };
        scratch
            .visible_surface_entities_by_main_entity
            .insert(*visible_main_entity, *render_entity);
        if scratch
            .seen_attached_root_parents
            .insert(attached_root_parent)
        {
            scratch
                .visible_attached_root_parents
                .push(attached_root_parent);
        }
    }
}

fn sort_attached_root_parents_by_z_order(
    executor: &LayerPoolRenderExecutor,
    attached_root_parents: &mut [MainEntity],
) {
    attached_root_parents.sort_unstable_by(|left, right| {
        let left_z_order = executor
            .parent_z_order_for_attached_root_parent(*left)
            .expect("visible attached root parent should keep a retained z order");
        let right_z_order = executor
            .parent_z_order_for_attached_root_parent(*right)
            .expect("visible attached root parent should keep a retained z order");
        left_z_order
            .total_cmp(&right_z_order)
            .then_with(|| left.cmp(right))
    });
}

fn prepare_ordered_transparent_item(
    render_meshes: &RenderAssets<RenderMesh>,
    render_materials: &RenderAssets<PreparedMaterial2d<UiDomBaseMaterial>>,
    render_mesh_instances: &mut RenderMesh2dInstances,
    render_material_instances: &RenderMaterial2dInstances<UiDomBaseMaterial>,
    main_entity: MainEntity,
    render_entity: Entity,
    pipeline_id: CachedRenderPipelineId,
) -> Option<Transparent2d> {
    let material_asset_id = *render_material_instances.get(&main_entity)?;
    let mesh_instance = render_mesh_instances.get_mut(&main_entity)?;
    let mesh = render_meshes.get(mesh_instance.mesh_asset_id)?;
    let material = render_materials.get(material_asset_id)?;

    mesh_instance.material_bind_group_id = material.get_bind_group_id();

    Some(Transparent2d {
        entity: (render_entity, main_entity),
        draw_function: material.properties.draw_function_id,
        pipeline: pipeline_id,
        sort_key: FloatOrd(0.0),
        batch_range: 0..1,
        extra_index: PhaseItemExtraIndex::None,
        extracted_index: usize::MAX,
        indexed: mesh.indexed(),
    })
}

fn prepare_ui_dom_ordered_surfaces(
    mut batched_instance_buffer: ResMut<BatchedInstanceBuffer<Mesh2dUniform>>,
    mut ordered_items_by_view: ResMut<OrderedTransparentUiDomSurfacesByView>,
    param: StaticSystemParam<<Mesh2dPipeline as GetBatchData>::Param>,
) {
    let system_param_item = param.into_inner();

    for ordered_items in ordered_items_by_view.items_by_view.values_mut() {
        ordered_items.retain_mut(|item| {
            let Some((buffer_data, _)) = <Mesh2dPipeline as GetBatchData>::get_batch_data(
                &system_param_item,
                (item.entity(), item.main_entity()),
            ) else {
                return false;
            };

            let buffer_index = batched_instance_buffer.push(buffer_data);
            item.batch_range = buffer_index.index..(buffer_index.index + 1);
            item.extra_index =
                PhaseItemExtraIndex::maybe_dynamic_offset(buffer_index.dynamic_offset);
            true
        });
    }
}

fn render_ordered_transparent_items<'w>(
    ordered_items: &[Transparent2d],
    render_pass: &mut TrackedRenderPass<'w>,
    world: &'w World,
    view_entity: Entity,
) -> Result<(), DrawError> {
    let draw_functions = world.resource::<DrawFunctions<Transparent2d>>();
    let mut draw_functions = draw_functions.write();
    draw_functions.prepare(world);

    for ordered_item in ordered_items {
        let Some(draw_function) = draw_functions.get_mut(ordered_item.draw_function()) else {
            continue;
        };
        draw_function.draw(world, render_pass, view_entity, ordered_item)?;
    }

    Ok(())
}

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

#[cfg(test)]
mod tests {
    use std::{any::TypeId, collections::HashMap};

    use bevy::{
        prelude::{Entity, Mesh2d},
        render::{sync_world::MainEntity, view::RenderVisibleEntities},
    };

    use super::*;

    #[test]
    fn collect_visible_ui_dom_surfaces_and_attached_root_parents_collects_unique_parents() {
        let first_root_parent: MainEntity = Entity::from_bits(1).into();
        let second_root_parent: MainEntity = Entity::from_bits(2).into();
        let first_surface_a: MainEntity = Entity::from_bits(10).into();
        let first_surface_b: MainEntity = Entity::from_bits(11).into();
        let second_surface: MainEntity = Entity::from_bits(20).into();
        let mut executor = LayerPoolRenderExecutor::new(LayerConfig::default());
        executor.replace_root_surface_order_for_test(
            first_root_parent,
            5.0,
            vec![first_surface_a, first_surface_b],
        );
        executor.replace_root_surface_order_for_test(second_root_parent, 1.0, vec![second_surface]);

        let mut visible_entities = RenderVisibleEntities::default();
        visible_entities.entities.insert(
            TypeId::of::<Mesh2d>(),
            vec![
                (Entity::from_bits(201), second_surface),
                (Entity::from_bits(101), first_surface_b),
                (Entity::from_bits(100), first_surface_a),
            ],
        );

        let mut scratch = VisibleUiDomSurfaceScratch::default();
        collect_visible_ui_dom_surfaces_and_attached_root_parents(
            &visible_entities,
            &executor,
            &mut scratch,
        );

        assert_eq!(
            scratch.visible_attached_root_parents,
            vec![second_root_parent, first_root_parent]
        );
        assert_eq!(
            scratch
                .visible_surface_entities_by_main_entity
                .get(&second_surface),
            Some(&Entity::from_bits(201))
        );
        assert_eq!(
            scratch
                .visible_surface_entities_by_main_entity
                .get(&first_surface_b),
            Some(&Entity::from_bits(101))
        );
    }

    #[test]
    fn collect_visible_ui_dom_surfaces_and_attached_root_parents_reuses_scratch_capacity() {
        let root_parent: MainEntity = Entity::from_bits(1).into();
        let first_surface: MainEntity = Entity::from_bits(10).into();
        let second_surface: MainEntity = Entity::from_bits(11).into();
        let mut executor = LayerPoolRenderExecutor::new(LayerConfig::default());
        executor.replace_root_surface_order_for_test(
            root_parent,
            3.0,
            vec![first_surface, second_surface],
        );

        let mut visible_entities = RenderVisibleEntities::default();
        visible_entities.entities.insert(
            TypeId::of::<Mesh2d>(),
            vec![
                (Entity::from_bits(100), first_surface),
                (Entity::from_bits(101), second_surface),
            ],
        );

        let mut scratch = VisibleUiDomSurfaceScratch::default();
        collect_visible_ui_dom_surfaces_and_attached_root_parents(
            &visible_entities,
            &executor,
            &mut scratch,
        );
        let first_visible_capacity = scratch.visible_surface_entities_by_main_entity.capacity();
        let first_parent_capacity = scratch.visible_attached_root_parents.capacity();
        let first_seen_capacity = scratch.seen_attached_root_parents.capacity();

        collect_visible_ui_dom_surfaces_and_attached_root_parents(
            &visible_entities,
            &executor,
            &mut scratch,
        );

        assert_eq!(
            scratch.visible_surface_entities_by_main_entity.capacity(),
            first_visible_capacity
        );
        assert_eq!(
            scratch.visible_attached_root_parents.capacity(),
            first_parent_capacity
        );
        assert_eq!(
            scratch.seen_attached_root_parents.capacity(),
            first_seen_capacity
        );
    }

    #[test]
    fn collect_visible_ui_dom_surfaces_and_attached_root_parents_skip_non_ui_dom_meshes() {
        let root_parent: MainEntity = Entity::from_bits(1).into();
        let ui_dom_surface: MainEntity = Entity::from_bits(10).into();
        let unrelated_mesh: MainEntity = Entity::from_bits(20).into();
        let mut executor = LayerPoolRenderExecutor::new(LayerConfig::default());
        executor.replace_root_surface_order_for_test(root_parent, 2.0, vec![ui_dom_surface]);

        let mut visible_entities = RenderVisibleEntities::default();
        visible_entities.entities.insert(
            TypeId::of::<Mesh2d>(),
            vec![
                (Entity::from_bits(100), unrelated_mesh),
                (Entity::from_bits(101), ui_dom_surface),
            ],
        );

        let mut scratch = VisibleUiDomSurfaceScratch::default();
        collect_visible_ui_dom_surfaces_and_attached_root_parents(
            &visible_entities,
            &executor,
            &mut scratch,
        );

        assert_eq!(scratch.visible_attached_root_parents, vec![root_parent]);
        assert_eq!(
            scratch.visible_surface_entities_by_main_entity,
            HashMap::from([(ui_dom_surface, Entity::from_bits(101))])
        );
    }

    #[test]
    fn sort_attached_root_parents_by_z_order_uses_parent_z_then_parent_id() {
        let first_root_parent: MainEntity = Entity::from_bits(1).into();
        let second_root_parent: MainEntity = Entity::from_bits(2).into();
        let third_root_parent: MainEntity = Entity::from_bits(3).into();
        let mut executor = LayerPoolRenderExecutor::new(LayerConfig::default());
        executor.replace_root_surface_order_for_test(first_root_parent, 4.0, vec![]);
        executor.replace_root_surface_order_for_test(second_root_parent, 1.0, vec![]);
        executor.replace_root_surface_order_for_test(third_root_parent, 4.0, vec![]);

        let mut attached_root_parents =
            vec![third_root_parent, first_root_parent, second_root_parent];
        sort_attached_root_parents_by_z_order(&executor, &mut attached_root_parents);

        assert_eq!(
            attached_root_parents,
            vec![second_root_parent, first_root_parent, third_root_parent]
        );
    }
}
