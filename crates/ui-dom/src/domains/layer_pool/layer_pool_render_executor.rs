//! `LayerPoolRenderExecutor` is the render-world owner that applies queued retained layer work.
//! This file exists so render-world GPU state, text/icon readiness, and pending retained applies stay out of the main-world `LayerPool` owner.
//!
//! The executor keeps its own frozen config snapshot and lazy render state because render ownership must not depend on live main-world mutation.
//! Pending font, text, and icon state are kept here so render-time readiness decisions stay near the code that consumes them.
//!
//! Strict invariants:
//! - render execution state belongs to the render world, not to `LayerPool`.
//! - the executor config is frozen for the executor lifetime.
//! - icon atlas readiness and GPU install decisions stay local to the executor boundary.
//! - pending applies are addressed by `LayerLease` and resolved only at render apply time.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Resource)]
pub(crate) struct LayerPoolRenderExecutor {
    pub(super) layer_config: FrozenLayerConfig,
    pub(super) icon_atlas_state: IconAtlasLoadState,
    pub(super) pending_applies: Vec<PendingLayerApply>,
    pub(super) pending_external_rgba_uploads: Vec<PendingExternalRgbaUpload>,
    ordered_surface_entities_by_attached_root_parent: HashMap<MainEntity, Vec<MainEntity>>,
    parent_z_order_by_attached_root_parent: HashMap<MainEntity, f32>,
    attached_root_parent_by_surface_entity: HashMap<MainEntity, MainEntity>,
    pub(super) pending_font_asset_loads: Vec<ExtractedFontAssetRecord>,
    pub(super) pending_required_font_asset_ids_by_family:
        HashMap<TextFontFamilyId, Vec<AssetId<UiDomFontAsset>>>,
    pub(super) pending_text_run_changes: Vec<ExtractedTextRunRecord>,
    pub(super) pending_text_run_removals: Vec<TextRunId>,
    pub(super) render: Option<LayerPoolRenderState>,
    pub(super) feedback: LayerPoolRenderFeedback,
}

impl LayerPoolRenderExecutor {
    pub(super) fn queue_font_asset_load(&mut self, font_asset: ExtractedFontAssetRecord) {
        if self
            .pending_font_asset_loads
            .iter()
            .any(|pending| pending.asset_id == font_asset.asset_id)
        {
            return;
        }
        self.pending_font_asset_loads.push(font_asset);
    }

    pub(super) fn set_required_font_assets(
        &mut self,
        required_font_asset_ids_by_family: HashMap<TextFontFamilyId, Vec<AssetId<UiDomFontAsset>>>,
    ) {
        self.pending_required_font_asset_ids_by_family = required_font_asset_ids_by_family;
    }

    pub(super) fn apply_text_run_change(&mut self, text_run_change: ExtractedTextRunRecord) {
        self.pending_text_run_removals
            .retain(|text_run_id| *text_run_id != text_run_change.text_run_id);
        if let Some(existing) = self
            .pending_text_run_changes
            .iter_mut()
            .find(|existing| existing.text_run_id == text_run_change.text_run_id)
        {
            *existing = text_run_change;
            return;
        }
        self.pending_text_run_changes.push(text_run_change);
    }

    pub(super) fn apply_text_run_removal(&mut self, text_run_id: TextRunId) {
        self.pending_text_run_changes
            .retain(|existing| existing.text_run_id != text_run_id);
        if self.pending_text_run_removals.contains(&text_run_id) {
            return;
        }
        self.pending_text_run_removals.push(text_run_id);
    }

    pub(super) fn apply_job(&mut self, job: LayerPoolRenderJob) {
        match job {
            LayerPoolRenderJob::Apply(apply) => {
                if let Some(existing) = self
                    .pending_applies
                    .iter_mut()
                    .find(|existing| existing.lease == apply.lease)
                {
                    *existing = apply;
                } else {
                    self.pending_applies.push(apply);
                }
            }
            LayerPoolRenderJob::ExternalRgbaUpload(upload) => {
                if let Some(existing) = self
                    .pending_external_rgba_uploads
                    .iter_mut()
                    .find(|existing| existing.lease == upload.lease)
                {
                    *existing = upload;
                } else {
                    self.pending_external_rgba_uploads.push(upload);
                }
            }
            LayerPoolRenderJob::Invalidate { lease } => {
                self.pending_applies.retain(|apply| apply.lease != lease);
                self.pending_external_rgba_uploads
                    .retain(|upload| upload.lease != lease);
            }
            LayerPoolRenderJob::ReplaceRootSurfaceOrder {
                attached_root_parent,
                parent_z_order,
                ordered_surface_entities,
            } => {
                self.replace_root_surface_order(
                    attached_root_parent,
                    parent_z_order,
                    ordered_surface_entities,
                );
            }
            LayerPoolRenderJob::DetachRootSurfaceOrder {
                attached_root_parent,
            } => {
                self.detach_root_surface_order(attached_root_parent);
            }
        }
    }

    fn replace_root_surface_order(
        &mut self,
        attached_root_parent: MainEntity,
        parent_z_order: f32,
        ordered_surface_entities: Vec<MainEntity>,
    ) {
        if let Some(existing_entities) = self
            .ordered_surface_entities_by_attached_root_parent
            .get(&attached_root_parent)
        {
            for surface_entity in existing_entities {
                let removed = self
                    .attached_root_parent_by_surface_entity
                    .remove(surface_entity);
                assert_eq!(
                    removed,
                    Some(attached_root_parent),
                    "surface-to-root order mapping should stay synchronized",
                );
            }
        }
        for surface_entity in &ordered_surface_entities {
            if let Some(existing_root_parent) = self
                .attached_root_parent_by_surface_entity
                .get(surface_entity)
            {
                assert_eq!(
                    *existing_root_parent, attached_root_parent,
                    "surface order updates should not reuse one surface entity across roots",
                );
                panic!("surface order updates should not contain duplicate surface entities");
            }
            self.attached_root_parent_by_surface_entity
                .insert(*surface_entity, attached_root_parent);
        }
        self.parent_z_order_by_attached_root_parent
            .insert(attached_root_parent, parent_z_order);
        self.ordered_surface_entities_by_attached_root_parent
            .insert(attached_root_parent, ordered_surface_entities);
    }

    fn detach_root_surface_order(&mut self, attached_root_parent: MainEntity) {
        let Some(existing_entities) = self
            .ordered_surface_entities_by_attached_root_parent
            .remove(&attached_root_parent)
        else {
            return;
        };
        let removed_parent_z_order = self
            .parent_z_order_by_attached_root_parent
            .remove(&attached_root_parent);
        assert!(
            removed_parent_z_order.is_some(),
            "root surface order z mapping should stay synchronized",
        );
        for surface_entity in existing_entities {
            let removed = self
                .attached_root_parent_by_surface_entity
                .remove(&surface_entity);
            assert_eq!(
                removed,
                Some(attached_root_parent),
                "surface-to-root order mapping should stay synchronized",
            );
        }
    }

    fn render_state(&mut self, device: &RenderDevice) -> &mut LayerPoolRenderState {
        if self.render.is_none() {
            self.render = Some(LayerPoolRenderState::new(device, self.layer_config));
            self.feedback.shared_render_state_created = self
                .feedback
                .shared_render_state_created
                .checked_add(1)
                .expect("shared render-state feedback should not overflow");
        }
        self.render
            .as_mut()
            .expect("render state should exist after lazy initialization")
    }

    pub(super) fn apply_pending(
        &mut self,
        gpu_images: &RenderAssets<GpuImage>,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        let pending_font_asset_loads = std::mem::take(&mut self.pending_font_asset_loads);
        let pending_required_font_asset_ids_by_family =
            std::mem::take(&mut self.pending_required_font_asset_ids_by_family);
        let pending_text_run_removals = std::mem::take(&mut self.pending_text_run_removals);
        let pending_text_run_changes = std::mem::take(&mut self.pending_text_run_changes);
        if !pending_font_asset_loads.is_empty()
            || !pending_required_font_asset_ids_by_family.is_empty()
            || !pending_text_run_removals.is_empty()
            || !pending_text_run_changes.is_empty()
        {
            let render_state = self.render_state(device);
            for (font_family_id, required_font_asset_ids) in
                pending_required_font_asset_ids_by_family
            {
                render_state
                    .text_run
                    .set_required_font_asset_ids(font_family_id, required_font_asset_ids);
            }
            for font_asset in pending_font_asset_loads {
                render_state.text_run.register_font_asset(font_asset);
            }
            for text_run_id in pending_text_run_removals {
                render_state.text_run.remove_run_record(text_run_id);
            }
            for text_run_change in pending_text_run_changes {
                render_state.text_run.apply_text_run_change(text_run_change);
            }
        }
        if let Some(pending_install) = self.icon_atlas_state.take_pending_install() {
            self.render_state(device)
                .install_ready_icon_atlas(device, queue, pending_install);
            self.icon_atlas_state = IconAtlasLoadState::Ready;
        }
        let pending_applies = std::mem::take(&mut self.pending_applies);
        for apply in pending_applies {
            let icon_atlas_ready = matches!(self.icon_atlas_state, IconAtlasLoadState::Ready)
                && self
                    .render
                    .as_ref()
                    .is_some_and(LayerPoolRenderState::icon_atlas_is_ready);
            if !apply.plan.icons.is_empty() && !icon_atlas_ready {
                self.pending_applies.push(apply);
                continue;
            }
            let Some(gpu_image) = gpu_images.get(&apply.image_handle) else {
                self.pending_applies.push(apply);
                continue;
            };
            assert!(
                gpu_image.size.width > 0 && gpu_image.size.height > 0,
                "ui-dom layer targets must have non-zero extents"
            );
            let pixel_density = self.layer_config.pixel_density();
            let render_apply_result = self.render_state(device).apply_plan_to_surface(
                device,
                queue,
                gpu_image,
                pixel_density,
                &apply,
            );
            if !render_apply_result.text_runs_ready {
                self.pending_applies.push(apply);
                continue;
            }
            if render_apply_result.rect_buffer_grew {
                self.feedback.shared_rect_buffer_grew = self
                    .feedback
                    .shared_rect_buffer_grew
                    .checked_add(1)
                    .expect("rect buffer growth feedback should not overflow");
            }
            self.feedback.repaint_applied = self
                .feedback
                .repaint_applied
                .checked_add(1)
                .expect("repaint feedback should not overflow");
            if !apply.plan.rects.is_empty() {
                self.feedback.rect_gpu_work_applied = self
                    .feedback
                    .rect_gpu_work_applied
                    .checked_add(1)
                    .expect("rect gpu work feedback should not overflow");
            }
        }
        let pending_external_rgba_uploads = std::mem::take(&mut self.pending_external_rgba_uploads);
        for upload in pending_external_rgba_uploads {
            let Some(gpu_image) = gpu_images.get(&upload.image_handle) else {
                self.pending_external_rgba_uploads.push(upload);
                continue;
            };
            apply_external_rgba_upload_to_surface(queue, gpu_image, &upload);
        }
    }

    pub(crate) fn trim_text_render_maintenance(&mut self) {
        let Some(render_state) = self.render.as_mut() else {
            return;
        };
        render_state.text_run.trim_shape_run_cache();
    }
}

fn apply_external_rgba_upload_to_surface(
    queue: &RenderQueue,
    gpu_image: &GpuImage,
    upload: &PendingExternalRgbaUpload,
) {
    assert_eq!(
        gpu_image.size.width, upload.image_extent_px.x,
        "external layer upload width should match the retained target"
    );
    assert_eq!(
        gpu_image.size.height, upload.image_extent_px.y,
        "external layer upload height should match the retained target"
    );
    assert_eq!(
        upload.rgba_bytes.len(),
        external_rgba_byte_len(upload.image_extent_px),
        "external layer upload bytes should match the retained target extent"
    );
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &gpu_image.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &upload.rgba_bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(upload.image_extent_px.x.saturating_mul(4)),
            rows_per_image: Some(upload.image_extent_px.y),
        },
        Extent3d {
            width: upload.image_extent_px.x,
            height: upload.image_extent_px.y,
            depth_or_array_layers: 1,
        },
    );
}

impl LayerPoolRenderExecutor {
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn pixel_density(&self) -> PixelDensity {
        self.layer_config.pixel_density()
    }

    #[cfg(test)]
    pub(crate) fn replace_root_surface_order_for_test(
        &mut self,
        attached_root_parent: MainEntity,
        parent_z_order: f32,
        ordered_surface_entities: Vec<MainEntity>,
    ) {
        self.replace_root_surface_order(
            attached_root_parent,
            parent_z_order,
            ordered_surface_entities,
        );
    }

    pub(crate) fn new(layer_config: LayerConfig) -> Self {
        Self {
            layer_config: FrozenLayerConfig::new(layer_config),
            icon_atlas_state: IconAtlasLoadState::Empty,
            pending_applies: Vec::new(),
            pending_external_rgba_uploads: Vec::new(),
            ordered_surface_entities_by_attached_root_parent: HashMap::new(),
            parent_z_order_by_attached_root_parent: HashMap::new(),
            attached_root_parent_by_surface_entity: HashMap::new(),
            pending_font_asset_loads: Vec::new(),
            pending_required_font_asset_ids_by_family: HashMap::new(),
            pending_text_run_changes: Vec::new(),
            pending_text_run_removals: Vec::new(),
            render: None,
            feedback: LayerPoolRenderFeedback::default(),
        }
    }

    pub(crate) fn ordered_surface_entities_for_attached_root_parent(
        &self,
        attached_root_parent: MainEntity,
    ) -> Option<&[MainEntity]> {
        self.ordered_surface_entities_by_attached_root_parent
            .get(&attached_root_parent)
            .map(Vec::as_slice)
    }

    pub(crate) fn parent_z_order_for_attached_root_parent(
        &self,
        attached_root_parent: MainEntity,
    ) -> Option<f32> {
        self.parent_z_order_by_attached_root_parent
            .get(&attached_root_parent)
            .copied()
    }

    pub(crate) fn attached_root_parent_for_surface_entity(
        &self,
        surface_entity: MainEntity,
    ) -> Option<MainEntity> {
        self.attached_root_parent_by_surface_entity
            .get(&surface_entity)
            .copied()
    }

    pub(crate) fn has_root_surface_orders(&self) -> bool {
        !self
            .ordered_surface_entities_by_attached_root_parent
            .is_empty()
    }
}
