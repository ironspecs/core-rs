//! `LayerPool` is the authoritative retained-layer owner for `ui-dom`.
//! This file exists so retained surface lifetime, retained mapping, and world-scoped apply entrypoints stay in one owner instead of leaking across DOM, paint, and host code.
//!
//! The type keeps setup policy as a frozen config snapshot and keeps render submission as staged ops handed to the `RenderOpsHandler` rather than performing GPU work directly in main-world paths.
//! The public world-scoped facade methods are kept here so callers use one unsurprising boundary instead of reaching into helper structs or sibling files.
//!
//! Strict invariants:
//! - `LayerPool` is the only retained backing-image allocator in `ui-dom`.
//! - `LayerPool` is the only owner of retained float-to-int mapping and retained layer bookkeeping.
//! - retained apply work must be addressed by `LayerLease`, never by raw `Handle<Image>`.
//! - `LayerPool` must not store duplicated authored plans or CPU-side translated render payloads.
//! - retained config is frozen for the lifetime of the installed `LayerPool`.

#[allow(clippy::wildcard_imports)]
use super::*;

/// Authoritative retained layer bookkeeping resource for retained surface reuse and GPU apply.
#[derive(Resource)]
pub struct LayerPool {
    pub(super) layer_config: FrozenLayerConfig,
    pub(super) layers: SlotMap<LayerId, LayerRecord>,
    pub(super) free_surfaces_by_size: BTreeMap<(u32, u32), Vec<Handle<Image>>>,
    pub(super) text_runs: SlotMap<TextRunId, CanonicalTextRunRecord>,
    pub(super) text_run_ids_by_signature:
        LruCache<TextRunSignature, TextRunId, UI_TEXT_RUN_SIGNATURE_CACHE_CAPACITY>,
    pub(super) pending_text_run_changes: HashMap<TextRunId, ExtractedTextRunRecord>,
    pub(super) pending_text_run_removals: Vec<TextRunId>,
    pub(super) metrics: LayerPoolMetricStore,
    pub(super) next_generation: LayerGeneration,
}

/// Validated paint target for one live layer: resolved surface, retained
/// mapping, and policy. `Ok(None)` means the layer's policy suppresses uploads.
#[derive(Clone, Debug)]
pub struct LayerPaintTarget {
    /// Resolved surface the repaint draws into.
    pub image_handle: Handle<Image>,
    /// Surface-local origin of this layer's zero point, in image pixels.
    pub layer_zero_in_image_px: [f32; 2],
    /// Retained upload policy for this layer.
    pub policy: LayerPoolPolicy,
}

impl LayerPool {
    pub(crate) fn resolve_text_run_id_from_hint(
        &mut self,
        text_run_signature: &TextRunSignature,
        text_run_id_hint: Option<TextRunId>,
    ) -> TextRunId {
        if let Some(text_run_id_hint) = text_run_id_hint
            && self
                .text_runs
                .get(text_run_id_hint)
                .is_some_and(|record| &record.signature == text_run_signature)
        {
            self.cache_text_run_signature(text_run_signature, text_run_id_hint);
            return text_run_id_hint;
        }
        if let Some(text_run_id) = self
            .text_run_ids_by_signature
            .get(text_run_signature)
            .copied()
        {
            let record = self
                .text_runs
                .get(text_run_id)
                .expect("signature cache should reference a live canonical text run");
            assert!(
                &record.signature == text_run_signature,
                "signature cache should resolve to the matching canonical text run"
            );
            return text_run_id;
        }
        let text_run_id = self.text_runs.insert(CanonicalTextRunRecord {
            signature: text_run_signature.clone(),
        });
        self.pending_text_run_removals
            .retain(|removed| *removed != text_run_id);
        self.pending_text_run_changes.insert(
            text_run_id,
            ExtractedTextRunRecord {
                text_run_id,
                text: text_run_signature.text.clone(),
                language_hint_id: text_run_signature.language_hint_id,
                family: text_run_signature.family.clone(),
                weight: text_run_signature.weight,
                style: text_run_signature.style,
            },
        );
        self.cache_text_run_signature(text_run_signature, text_run_id);
        text_run_id
    }

    fn cache_text_run_signature(
        &mut self,
        text_run_signature: &TextRunSignature,
        text_run_id: TextRunId,
    ) {
        let evicted = self
            .text_run_ids_by_signature
            .insert_and_evict(text_run_signature, text_run_id);
        let Some((evicted_signature, evicted_text_run_id)) = evicted else {
            return;
        };
        if &evicted_signature == text_run_signature {
            return;
        }
        let removed = self.text_runs.remove(evicted_text_run_id);
        assert!(
            removed.is_some(),
            "evicted text-run cache entry should reference a live canonical text run"
        );
        self.pending_text_run_changes.remove(&evicted_text_run_id);
        if !self
            .pending_text_run_removals
            .contains(&evicted_text_run_id)
        {
            self.pending_text_run_removals.push(evicted_text_run_id);
        }
    }

    pub(super) fn drain_pending_text_run_changes(&mut self) -> Vec<ExtractedTextRunRecord> {
        std::mem::take(&mut self.pending_text_run_changes)
            .into_values()
            .collect()
    }

    pub(super) fn drain_pending_text_run_removals(&mut self) -> Vec<TextRunId> {
        std::mem::take(&mut self.pending_text_run_removals)
    }

    pub(super) fn create_layer(&mut self, layer_rect: LayerRect) -> LayerLease {
        let mapping = layer_mapping_for_rect(layer_rect, self.layer_config.pixel_density());
        let (image_width_px, image_height_px) = texture_extent_px_from_layer_mapping(mapping);
        let generation = self.allocate_generation();
        let layer_id = self.layers.insert(LayerRecord {
            generation,
            policy: LayerPoolPolicy::Normal,
            image_width_px,
            image_height_px,
            layer_zero_x_in_image_px: mapping.layer_zero_x_in_image_px,
            layer_zero_y_in_image_px: mapping.layer_zero_y_in_image_px,
            image: None,
        });
        self.metrics.increment(LayerPoolMetricName::LayerCreated);
        LayerLease {
            layer_id,
            generation,
        }
    }

    fn update_layer(
        &mut self,
        lease: LayerLease,
        layer_rect: LayerRect,
    ) -> Result<bool, &'static str> {
        let mapping = layer_mapping_for_rect(layer_rect, self.layer_config.pixel_density());
        let (image_width_px, image_height_px) = texture_extent_px_from_layer_mapping(mapping);
        self.validate_current_lease(lease, LayerPoolMetricName::StaleLeaseRejected)?;
        let record = self
            .layers
            .get(lease.layer_id)
            .expect("validated lease should resolve to a record");
        let surface_grew =
            image_width_px > record.image_width_px || image_height_px > record.image_height_px;
        let mapping_changed = image_width_px != record.image_width_px
            || image_height_px != record.image_height_px
            || mapping.layer_zero_x_in_image_px.to_bits()
                != record.layer_zero_x_in_image_px.to_bits()
            || mapping.layer_zero_y_in_image_px.to_bits()
                != record.layer_zero_y_in_image_px.to_bits();
        if !mapping_changed {
            return Ok(false);
        }
        let released_surface = if surface_grew {
            let record = self
                .layers
                .get_mut(lease.layer_id)
                .expect("validated lease should still resolve to a record");
            let released_image = record.image.take();
            let released_width_px = record.image_width_px;
            let released_height_px = record.image_height_px;
            released_image.map(|image_handle| (released_width_px, released_height_px, image_handle))
        } else {
            None
        };
        if let Some((released_width_px, released_height_px, image_handle)) = released_surface {
            self.release_image_to_free_size(released_width_px, released_height_px, image_handle);
            self.metrics.increment(LayerPoolMetricName::SurfaceReleased);
        }
        let record = self
            .layers
            .get_mut(lease.layer_id)
            .expect("validated lease should still resolve to a record");
        if surface_grew {
            record.image_width_px = image_width_px;
            record.image_height_px = image_height_px;
        }
        record.layer_zero_x_in_image_px = mapping.layer_zero_x_in_image_px;
        record.layer_zero_y_in_image_px = mapping.layer_zero_y_in_image_px;
        self.metrics
            .increment(LayerPoolMetricName::LayerMappingUpdated);
        if surface_grew {
            self.metrics.increment(LayerPoolMetricName::SurfaceGrew);
        }
        Ok(true)
    }

    pub(super) fn destroy_layer(&mut self, lease: LayerLease) -> Result<(), &'static str> {
        self.validate_current_lease(lease, LayerPoolMetricName::StaleLeaseRejected)?;
        let removed = self
            .layers
            .remove(lease.layer_id)
            .expect("validated layer lease should remove an existing slot");
        if let Some(image_handle) = removed.image {
            self.release_image_to_free_size(
                removed.image_width_px,
                removed.image_height_px,
                image_handle,
            );
            self.metrics.increment(LayerPoolMetricName::SurfaceReleased);
        }
        self.metrics.increment(LayerPoolMetricName::LayerDestroyed);
        Ok(())
    }

    fn set_layer_policy(
        &mut self,
        lease: LayerLease,
        policy: LayerPoolPolicy,
    ) -> Result<bool, &'static str> {
        self.validate_current_lease(lease, LayerPoolMetricName::StaleLeaseRejected)?;
        let record = self
            .layers
            .get_mut(lease.layer_id)
            .expect("validated lease should still resolve to a record");
        if record.policy == policy {
            return Ok(false);
        }
        record.policy = policy;
        self.metrics.increment(LayerPoolMetricName::PolicyChanged);
        Ok(true)
    }

    fn acquire_surface(
        &mut self,
        world: &mut World,
        lease: LayerLease,
    ) -> Result<(), &'static str> {
        self.validate_current_lease(lease, LayerPoolMetricName::StaleLeaseRejected)?;
        let record = self
            .layers
            .get(lease.layer_id)
            .expect("validated lease should resolve to a record");
        if record.image.is_some() {
            return Ok(());
        }
        let width_px = record.image_width_px;
        let height_px = record.image_height_px;
        let reused_surface = self.take_free_surface(width_px, height_px);
        let reusing_surface = reused_surface.is_some();
        let image_handle = reused_surface
            .unwrap_or_else(|| create_layer_surface_image(world, (width_px, height_px)));
        let record = self
            .layers
            .get_mut(lease.layer_id)
            .expect("validated lease should still resolve to a record");
        record.image = Some(image_handle);
        self.metrics.increment(if reusing_surface {
            LayerPoolMetricName::SurfaceReused
        } else {
            LayerPoolMetricName::SurfaceAllocated
        });
        Ok(())
    }

    fn release_surface(&mut self, lease: LayerLease) -> Result<(), &'static str> {
        self.validate_current_lease(lease, LayerPoolMetricName::StaleLeaseRejected)?;
        let record = self
            .layers
            .get_mut(lease.layer_id)
            .expect("validated lease should still resolve to a record");
        let Some(image_handle) = record.image.take() else {
            return Ok(());
        };
        let width_px = record.image_width_px;
        let height_px = record.image_height_px;
        self.release_image_to_free_size(width_px, height_px, image_handle);
        self.metrics.increment(LayerPoolMetricName::SurfaceReleased);
        Ok(())
    }

    /// Returns the aggregate count for one named `LayerPool` metric.
    ///
    /// This API intentionally takes `LayerPoolMetricName` directly so callers
    /// can reach for metrics first without paying for string lookup or relying
    /// on hidden slot order. This is the primary metric read API.
    #[must_use]
    pub fn count(&self, name: LayerPoolMetricName) -> u64 {
        self.metrics.count(name)
    }

    /// Returns one snapshot copy of the full named metric tally.
    ///
    /// This method only clones the internal fixed enum-to-count tally and
    /// returns that clone by value. It does not rebuild pairs, look up strings,
    /// or reinterpret slot order. This is a secondary profiling API; prefer
    /// `count(name)` for direct reads.
    #[must_use]
    pub fn metrics(&self) -> LayerPoolMetrics {
        self.metrics.metrics()
    }

    /// Returns retained metric history in oldest-to-newest order.
    ///
    /// History returns named retained entries for the same reason as `count()`:
    /// metrics should be an obvious first debugging tool without any string
    /// conversion or slot-index folklore.
    #[must_use]
    pub fn history(&self) -> LayerPoolMetricHistoryIter<'_> {
        self.metrics.history()
    }

    /// Clears named counters and retained history, then starts a fresh profiling window.
    ///
    /// This stays on `LayerPool` so metric lifecycle remains attached to the
    /// retained owner instead of leaking into helper types.
    #[allow(dead_code)]
    pub(crate) fn reset_metrics(&mut self) {
        self.metrics.reset();
    }

    /// Increments one `LayerPool` metric counter.
    ///
    /// Mirrors `RootDom::increment_metric`'s contract: call only after the
    /// measured work actually occurred, never as bookkeeping ahead of it.
    pub(crate) fn increment_metric(&mut self, name: LayerPoolMetricName) {
        self.metrics.increment(name);
    }

    /// Creates one retained layer lease through the authoritative world-scoped facade.
    #[must_use]
    pub fn create_layer_lease_in_world(
        world: &mut World,
        layer_rect_px: (f32, f32, f32, f32),
    ) -> LayerLease {
        let layer_rect = LayerRect::from_tuple(layer_rect_px);
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool.create_layer(layer_rect)
        })
    }

    /// Updates one retained layer lease through the authoritative world-scoped facade.
    pub fn update_layer_lease_in_world(
        world: &mut World,
        lease: LayerLease,
        layer_rect_px: (f32, f32, f32, f32),
    ) -> Result<(), &'static str> {
        let layer_rect = LayerRect::from_tuple(layer_rect_px);
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        let (mapping_changed, policy) =
            world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
                let mapping_changed = layer_pool.update_layer(lease, layer_rect)?;
                let policy = layer_pool
                    .layers
                    .get(lease.layer_id)
                    .expect("validated lease should still resolve to a record")
                    .policy;
                Ok::<(bool, LayerPoolPolicy), &'static str>((mapping_changed, policy))
            })?;
        if mapping_changed {
            enqueue_layer_invalidation_for_policy(world, lease, policy);
        }
        Ok(())
    }

    /// Destroys one retained layer lease through the authoritative world-scoped facade.
    pub fn destroy_layer_lease_in_world(
        world: &mut World,
        lease: LayerLease,
    ) -> Result<(), &'static str> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool.destroy_layer(lease)
        })?;
        enqueue_layer_invalidation(world, lease);
        Ok(())
    }

    /// Updates one retained layer policy through the authoritative world-scoped facade.
    pub fn set_layer_policy_in_world(
        world: &mut World,
        lease: LayerLease,
        policy: LayerPoolPolicy,
    ) -> Result<(), &'static str> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        let policy_changed =
            world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
                layer_pool.set_layer_policy(lease, policy)
            })?;
        if policy_changed {
            enqueue_layer_invalidation(world, lease);
        }
        Ok(())
    }

    /// Acquires one retained layer surface and returns its caller-renderable target.
    pub fn acquire_external_layer_target_in_world(
        world: &mut World,
        lease: LayerLease,
    ) -> Result<ExternalLayerTarget, &'static str> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|world, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool.acquire_surface(world, lease)?;
            let record = layer_pool
                .layers
                .get(lease.layer_id)
                .expect("validated lease should still resolve to a record");
            Ok(ExternalLayerTarget {
                lease,
                image_handle: record
                    .image
                    .clone()
                    .expect("acquired layer surface should bind an image handle"),
                image_extent_px: bevy::math::UVec2::new(
                    record.image_width_px,
                    record.image_height_px,
                ),
                origin_offset_px: [
                    record.layer_zero_x_in_image_px,
                    record.layer_zero_y_in_image_px,
                ],
            })
        })
    }

    /// Submits one full RGBA upload for a caller-rendered external layer target.
    ///
    /// The caller keeps ownership of its source buffer. `LayerPool` copies the
    /// slice once into the supplied render-ops handler, and the render executor
    /// uploads those bytes into the target texture. The supplied handler must be
    /// registered for the per-tick extract drain (see `register_render_ops_handler`);
    /// uploads staged into an unregistered handler never reach the executor.
    pub fn submit_external_rgba_in_world(
        world: &mut World,
        handler: &crate::domains::RenderOpsHandler,
        target: &ExternalLayerTarget,
        rgba_bytes: &[u8],
    ) -> Result<(), &'static str> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        assert_eq!(
            rgba_bytes.len(),
            external_rgba_byte_len(target.image_extent_px),
            "external layer upload bytes should match the retained target extent"
        );
        {
            let layer_pool = world.resource::<LayerPool>();
            let record = layer_pool
                .layers
                .get(target.lease.layer_id)
                .ok_or("external layer lease should resolve")?;
            if record.generation != target.lease.generation {
                return Err("external layer lease generation should match");
            }
            if record.policy != LayerPoolPolicy::External {
                return Err("external layer target should use external policy");
            }
            if record.image.as_ref() != Some(&target.image_handle) {
                return Err("external layer target image handle should match the live layer");
            }
            if record.image_width_px != target.image_extent_px.x
                || record.image_height_px != target.image_extent_px.y
            {
                return Err("external layer target extent should match the live layer");
            }
            Ok::<(), &'static str>(())
        }?;
        handler.stage_external_rgba(crate::domains::StagedExternalRgbaUpload {
            lease: target.lease,
            image_handle: target.image_handle.clone(),
            image_extent_px: target.image_extent_px,
            rgba_bytes: rgba_bytes.to_vec(),
        });
        Ok(())
    }

    /// Ensures one retained layer lease owns a compatible retained surface.
    pub fn acquire_layer_surface_in_world(
        world: &mut World,
        lease: LayerLease,
    ) -> Result<(), &'static str> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|world, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool.acquire_surface(world, lease)
        })
    }

    /// Releases the current retained surface for one lease back to the free exact-size pool.
    pub fn release_layer_surface_in_world(
        world: &mut World,
        lease: LayerLease,
    ) -> Result<(), &'static str> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool.release_surface(lease)
        })?;
        enqueue_layer_invalidation(world, lease);
        Ok(())
    }

    pub(crate) fn cloned_surface_image_in_world(
        world: &mut World,
        lease: LayerLease,
    ) -> Result<Handle<Image>, &'static str> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|world, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool.acquire_surface(world, lease)?;
            let record = layer_pool
                .layers
                .get(lease.layer_id)
                .expect("validated lease should still resolve to a record");
            Ok(record
                .image
                .clone()
                .expect("acquired layer surface should bind an image handle"))
        })
    }

    /// Validates one lease and acquires its retained surface for a coming repaint.
    ///
    /// Order is load-bearing and matches the deleted apply path exactly:
    /// validate lease -> read policy -> suppress Offscreen/External (`UploadSuppressed`
    /// metric) -> require the direct GPU executor -> acquire surface.
    pub fn acquire_paint_target_in_world(
        world: &mut World,
        lease: LayerLease,
    ) -> Result<Option<LayerPaintTarget>, LayerPoolApplyError> {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|world, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool
                .validate_current_lease(lease, LayerPoolMetricName::StaleLeaseRejected)
                .map_err(LayerPoolApplyError::InvalidLease)?;
            let policy = layer_pool
                .layers
                .get(lease.layer_id)
                .expect("validated lease should still resolve to a record")
                .policy;
            if policy == LayerPoolPolicy::Offscreen || policy == LayerPoolPolicy::External {
                layer_pool
                    .metrics
                    .increment(LayerPoolMetricName::UploadSuppressed);
                return Ok(None);
            }
            require_direct_gpu_executor(world)?;
            layer_pool
                .acquire_surface(world, lease)
                .map_err(LayerPoolApplyError::InvalidLease)?;
            let record = layer_pool
                .layers
                .get(lease.layer_id)
                .expect("validated lease should still resolve to a record");
            Ok(Some(LayerPaintTarget {
                image_handle: record
                    .image
                    .clone()
                    .expect("acquired layer surface should bind an image handle"),
                layer_zero_in_image_px: [
                    record.layer_zero_x_in_image_px,
                    record.layer_zero_y_in_image_px,
                ],
                policy,
            }))
        })
    }

    fn allocate_generation(&mut self) -> LayerGeneration {
        let generation = self.next_generation;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layer generation should not overflow");
        generation
    }

    fn take_free_surface(&mut self, width_px: u32, height_px: u32) -> Option<Handle<Image>> {
        let free_surfaces = self.free_surfaces_by_size.get_mut(&(width_px, height_px))?;
        free_surfaces.pop()
    }

    fn release_image_to_free_size(
        &mut self,
        width_px: u32,
        height_px: u32,
        image_handle: Handle<Image>,
    ) {
        self.free_surfaces_by_size
            .entry((width_px, height_px))
            .or_default()
            .push(image_handle);
    }

    fn validate_current_lease(
        &mut self,
        lease: LayerLease,
        stale_metric_name: LayerPoolMetricName,
    ) -> Result<(), &'static str> {
        let Some(record) = self.layers.get(lease.layer_id) else {
            self.metrics.increment(stale_metric_name);
            return Err("layer lease should reference an existing slot");
        };
        if record.generation != lease.generation {
            self.metrics.increment(stale_metric_name);
            return Err("layer lease generation should match the current slot generation");
        }
        Ok(())
    }
}

impl LayerPool {
    /// Creates one low-level retained-layer owner with one fixed density policy.
    ///
    /// App code should prefer `install_ui_dom_runtime_pair` so `LayerPool` and
    /// `LayerPoolRenderExecutor` are replaced together from the same config snapshot.
    #[must_use]
    pub fn new(layer_config: LayerConfig) -> Self {
        Self {
            layer_config: FrozenLayerConfig::new(layer_config),
            layers: SlotMap::with_key(),
            free_surfaces_by_size: BTreeMap::new(),
            text_runs: SlotMap::with_key(),
            text_run_ids_by_signature: LruCache::default(),
            pending_text_run_changes: HashMap::new(),
            pending_text_run_removals: Vec::new(),
            metrics: LayerPoolMetricStore::default(),
            next_generation: 1,
        }
    }

    #[must_use]
    pub(crate) const fn pixel_density(&self) -> PixelDensity {
        self.layer_config.pixel_density()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::image::Image;
    use bevy::prelude::{Assets, World};

    fn pool_with_lease(rect_px: (f32, f32, f32, f32)) -> (LayerPool, LayerLease) {
        let mut pool = LayerPool::new(LayerConfig::default());
        let lease = pool.create_layer(LayerRect::from_tuple(rect_px));
        (pool, lease)
    }

    #[test]
    fn validate_current_lease_rejects_destroyed_lease() {
        let (mut pool, lease) = pool_with_lease((0.0, 0.0, 10.0, 10.0));
        pool.destroy_layer(lease).expect("destroy should succeed");
        assert!(
            pool.validate_current_lease(lease, LayerPoolMetricName::StaleLeaseRejected)
                .is_err(),
            "a destroyed lease should fail validation"
        );
        assert_eq!(pool.count(LayerPoolMetricName::StaleLeaseRejected), 1);
    }

    #[test]
    fn update_layer_reports_mapping_change_only_on_change() {
        let (mut pool, lease) = pool_with_lease((0.0, 0.0, 10.0, 10.0));
        let unchanged = pool
            .update_layer(lease, LayerRect::from_tuple((0.0, 0.0, 10.0, 10.0)))
            .expect("same-rect update");
        assert!(
            !unchanged,
            "an unchanged mapping should not report a change"
        );
        let changed = pool
            .update_layer(lease, LayerRect::from_tuple((0.0, 0.0, 20.0, 10.0)))
            .expect("grown-rect update");
        assert!(changed, "a grown mapping should report a change");
        assert_eq!(pool.count(LayerPoolMetricName::LayerMappingUpdated), 1);
        assert_eq!(pool.count(LayerPoolMetricName::SurfaceGrew), 1);
    }

    #[test]
    fn set_layer_policy_reports_change_only_on_change() {
        let (mut pool, lease) = pool_with_lease((0.0, 0.0, 10.0, 10.0));
        let unchanged = pool
            .set_layer_policy(lease, LayerPoolPolicy::Normal)
            .expect("same-policy update");
        assert!(
            !unchanged,
            "setting the already-active policy should not report a change"
        );
        let changed = pool
            .set_layer_policy(lease, LayerPoolPolicy::BestEffort)
            .expect("policy change");
        assert!(changed, "a different policy should report a change");
        assert_eq!(pool.count(LayerPoolMetricName::PolicyChanged), 1);
    }

    #[test]
    fn destroy_layer_releases_surface_and_counts_metrics() {
        let mut world = World::new();
        world.insert_resource(Assets::<Image>::default());
        let (mut pool, lease) = pool_with_lease((0.0, 0.0, 10.0, 10.0));
        pool.acquire_surface(&mut world, lease)
            .expect("surface should acquire");
        pool.destroy_layer(lease).expect("destroy should succeed");
        assert_eq!(pool.count(LayerPoolMetricName::LayerDestroyed), 1);
        assert_eq!(pool.count(LayerPoolMetricName::SurfaceReleased), 1);
        assert_eq!(
            pool.free_surfaces_by_size.len(),
            1,
            "the released surface should be recorded under one exact-size key"
        );
        assert!(
            !pool
                .free_surfaces_by_size
                .values()
                .next()
                .expect("one size entry should exist")
                .is_empty(),
            "the released surface should be reusable"
        );
    }

    #[test]
    fn released_surface_is_reused_by_matching_extent() {
        let mut world = World::new();
        world.insert_resource(Assets::<Image>::default());
        let (mut pool, lease_a) = pool_with_lease((0.0, 0.0, 10.0, 10.0));
        pool.acquire_surface(&mut world, lease_a)
            .expect("lease a surface should acquire");
        assert_eq!(pool.count(LayerPoolMetricName::SurfaceAllocated), 1);
        pool.release_surface(lease_a)
            .expect("lease a surface should release");

        let lease_b = pool.create_layer(LayerRect::from_tuple((0.0, 0.0, 10.0, 10.0)));
        pool.acquire_surface(&mut world, lease_b)
            .expect("lease b surface should acquire");
        assert_eq!(pool.count(LayerPoolMetricName::SurfaceReused), 1);
        assert_eq!(pool.count(LayerPoolMetricName::SurfaceAllocated), 1);
    }
}
