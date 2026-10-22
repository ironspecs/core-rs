//! Render-owned text-run render state for the retained layer pool.
//!
//! This file owns the retained text-run state that caches glyph masks and text
//! buffers, registers fonts, and submits final text-run GPU draws. Its purpose
//! is to keep the text-run state struct beside its methods instead of growing
//! one mixed text-render owner file.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextRunRenderState` struct and its `impl` blocks
//! - font registration is read-only here; required asset ids are declared
//!   through the font registration state
//! - glyph and text-buffer caches are bounded by the shared layer-pool capacities

use std::collections::HashMap;

use bevy::asset::AssetId;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use collections::lru_cache::LruCache;
use cosmic_text::{FontSystem, SwashCache, fontdb::Source as FontSource};
use slotmap::SecondaryMap;
use wgpu;

use crate::domains::layer_ops::{
    TextAlign, TextFontFamilyId, TextRunId, TextRunPaintOp, TextWrapMode,
};
use crate::domains::style::computed::TextFamilyList;
use crate::domains::text_layout::{self, FontStyleKw, FontWeightKw};
use crate::render_plugin::UiDomFontAsset;
use crate::types::{DomWhiteSpace, PixelDensity};

use super::font_registration_state::FontRegistrationState;
use super::layer_pool_text_render::{
    CachedGlyphMask, CachedTextBuffer, GlyphMaskBuildResult, GlyphMaskKey, PreparedGlyphDraw,
    TEXT_SHAPE_RUN_CACHE_RETAINED_AGE_COUNT, TextBufferKey, TextRunSurfaceContext,
    apply_text_runs_to_surface, build_glyph_mask,
};
use super::preload_warmup_queue::PreloadWarmupQueue;
use super::{
    ExtractedFontAssetRecord, ExtractedTextRunRecord, LayerLease, LayerPipelineState,
    PendingLeaseDependencies, RenderTextRunRecord, UI_TEXT_BUFFER_CACHE_CAPACITY,
    UI_TEXT_GLYPH_CACHE_CAPACITY, UI_TEXT_PRELOAD_BATCH_SIZE,
};

pub(super) struct TextRunRenderState {
    pub(super) pixel_density: PixelDensity,
    pub(super) debug_pipeline: LayerPipelineState,
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) gpu_buffer: wgpu::Buffer,
    pub(super) gpu_capacity: usize,
    pub(super) uniform_buffer: wgpu::Buffer,
    pub(super) uniform_bind_group: wgpu::BindGroup,
    pub(super) sampler: wgpu::Sampler,
    pub(super) glyph_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) font_system: FontSystem,
    pub(super) swash_cache: SwashCache,
    pub(super) font_registration_state: FontRegistrationState,
    pub(super) run_bank: SecondaryMap<TextRunId, RenderTextRunRecord>,
    pub(super) text_buffer_cache:
        LruCache<TextBufferKey, CachedTextBuffer, UI_TEXT_BUFFER_CACHE_CAPACITY>,
    pub(super) glyph_mask_cache:
        LruCache<GlyphMaskKey, CachedGlyphMask, UI_TEXT_GLYPH_CACHE_CAPACITY>,
    pub(super) preload_warmup_queue: PreloadWarmupQueue,
    pub(super) pending_lease_dependencies: HashMap<LayerLease, PendingLeaseDependencies>,
    pub(super) prepared_glyph_draws: Vec<PreparedGlyphDraw>,
    pub(super) fallback_debug_runs: Vec<TextRunPaintOp>,
}

impl TextRunRenderState {
    pub(super) fn set_required_font_asset_ids(
        &mut self,
        font_family_id: TextFontFamilyId,
        required_font_asset_ids: Vec<AssetId<UiDomFontAsset>>,
    ) {
        self.font_registration_state
            .set_required_font_asset_ids(font_family_id, required_font_asset_ids);
    }

    pub(super) fn remove_run_record(&mut self, text_run_id: TextRunId) {
        self.run_bank.remove(text_run_id);
    }

    pub(super) fn insert_run_record(
        &mut self,
        text_run_id: TextRunId,
        run_record: RenderTextRunRecord,
    ) {
        self.run_bank.insert(text_run_id, run_record);
    }

    pub(super) fn apply_text_run_change(&mut self, text_run_change: ExtractedTextRunRecord) {
        self.insert_run_record(
            text_run_change.text_run_id,
            RenderTextRunRecord {
                text: text_run_change.text,
                language_hint_id: text_run_change.language_hint_id,
                font_family_id: text_run_change.font_family_id,
                family: text_run_change.family,
                weight: text_run_change.weight,
                style: text_run_change.style,
            },
        );
    }

    pub(super) fn trim_shape_run_cache(&mut self) {
        self.font_system
            .shape_run_cache
            .trim(TEXT_SHAPE_RUN_CACHE_RETAINED_AGE_COUNT);
    }

    pub(super) fn apply_to_surface(
        &mut self,
        context: &TextRunSurfaceContext<'_>,
        text_runs: &[TextRunPaintOp],
    ) -> bool {
        apply_text_runs_to_surface(self, context, text_runs)
    }

    pub(super) fn run_preload_warmups(&mut self, device: &RenderDevice, queue: &RenderQueue) {
        for request in self
            .preload_warmup_queue
            .take_batch(UI_TEXT_PRELOAD_BATCH_SIZE)
        {
            if !self
                .font_registration_state
                .ready(request.key.font_family_id)
            {
                self.preload_warmup_queue.push(request);
                continue;
            }
            let text_layout_request = text_layout::TextLayoutRequest {
                text: request.sample_text,
                white_space: DomWhiteSpace::Normal,
                word_break: crate::types::DomWordBreak::Normal,
                family: TextFamilyList::default(),
                language_hint_id: request.key.language_hint_id,
                weight: FontWeightKw::NORMAL,
                style: FontStyleKw::NORMAL,
                direction: crate::types::DomTextDirection::Ltr,
                font_size_px: f32::from_bits(request.key.font_size_bits),
                line_height_px: None,
                width_px: None,
                wrap_mode: TextWrapMode::WordOrGlyph,
                alignment: TextAlign::Left,
            };
            let buffer = text_layout::text_render::build_render_text_buffer(
                &mut self.font_system,
                &text_layout_request,
                None,
            );
            let raster_scale = f32::from_bits(request.key.raster_scale_bits);
            for run in buffer.layout_runs() {
                for glyph in run.glyphs {
                    let physical_glyph = glyph.physical((0.0, 0.0), raster_scale);
                    let glyph_key = GlyphMaskKey {
                        cache_key: physical_glyph.cache_key,
                    };
                    if self.glyph_mask_cache.contains(&glyph_key) {
                        let _ = self.glyph_mask_cache.get(&glyph_key);
                        continue;
                    }
                    let GlyphMaskBuildResult::Cached(cached_glyph_mask) = build_glyph_mask(
                        device,
                        queue,
                        &self.glyph_bind_group_layout,
                        &self.sampler,
                        &mut self.font_system,
                        &mut self.swash_cache,
                        physical_glyph.cache_key,
                    ) else {
                        continue;
                    };
                    let _ = self
                        .glyph_mask_cache
                        .preload(&glyph_key, || cached_glyph_mask);
                }
            }
        }
    }
}

impl TextRunRenderState {
    pub(super) fn register_font_asset(&mut self, font_asset: ExtractedFontAssetRecord) {
        if self
            .font_registration_state
            .registered_font_asset_ids
            .contains(&font_asset.asset_id)
        {
            return;
        }
        self.font_system
            .db_mut()
            .load_font_source(FontSource::Binary(std::sync::Arc::new(font_asset.bytes)));
        self.font_registration_state
            .mark_registered(font_asset.asset_id);
    }
}

#[cfg(test)]
use crate::domains::layer_ops::TextLanguageId;
#[cfg(test)]
use crate::domains::layer_pool::layer_pool_text_render::create_text_run_render_state;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::render::renderer::{RenderDevice, RenderQueue, WgpuWrapper};
    use bevy::tasks::block_on;
    use slotmap::SlotMap;

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

    fn fresh_text_run_render_state() -> (RenderDevice, TextRunRenderState) {
        let (render_device, _render_queue) = noop_render_device_and_queue();
        let state = create_text_run_render_state(&render_device, PixelDensity::X1);
        (render_device, state)
    }

    #[test]
    fn render_state_constructs_pipelines_and_starts_without_ready_family() {
        let (_render_device, state) = fresh_text_run_render_state();
        assert!(state.run_bank.is_empty());
        assert!(state.pending_lease_dependencies.is_empty());
        assert!(
            !state
                .font_registration_state
                .ready(TextFontFamilyId::UiSans)
        );
    }

    #[test]
    fn apply_text_run_change_populates_and_remove_clears_run_bank() {
        let (_render_device, mut state) = fresh_text_run_render_state();
        let mut text_run_ids = SlotMap::with_key();
        let text_run_id = text_run_ids.insert(());
        state.apply_text_run_change(ExtractedTextRunRecord {
            text_run_id,
            text: Arc::<str>::from("Settings"),
            language_hint_id: TextLanguageId::English,
            font_family_id: TextFontFamilyId::UiSans,
            family: TextFamilyList::default(),
            weight: FontWeightKw::NORMAL,
            style: FontStyleKw::Normal,
        });

        assert_eq!(
            state
                .run_bank
                .get(text_run_id)
                .map(|record| record.text.as_ref()),
            Some("Settings")
        );
        assert_eq!(
            state
                .run_bank
                .get(text_run_id)
                .map(|record| record.language_hint_id),
            Some(TextLanguageId::English)
        );

        state.remove_run_record(text_run_id);
        assert!(state.run_bank.get(text_run_id).is_none());
    }

    #[test]
    fn register_font_asset_makes_family_ready_when_required() {
        let (_render_device, mut state) = fresh_text_run_render_state();
        let mut assets = bevy::asset::Assets::<UiDomFontAsset>::default();
        let id_a = assets
            .add(UiDomFontAsset {
                bytes: Arc::from(&b"a"[..]),
            })
            .id();
        let id_b = assets
            .add(UiDomFontAsset {
                bytes: Arc::from(&b"b"[..]),
            })
            .id();

        state.set_required_font_asset_ids(TextFontFamilyId::UiSans, vec![id_a, id_b]);
        assert!(
            !state
                .font_registration_state
                .ready(TextFontFamilyId::UiSans)
        );

        state.register_font_asset(ExtractedFontAssetRecord {
            asset_id: id_a,
            bytes: Arc::from(&b"a"[..]),
        });
        assert!(
            !state
                .font_registration_state
                .ready(TextFontFamilyId::UiSans)
        );

        state.register_font_asset(ExtractedFontAssetRecord {
            asset_id: id_b,
            bytes: Arc::from(&b"b"[..]),
        });
        assert!(
            state
                .font_registration_state
                .ready(TextFontFamilyId::UiSans)
        );
    }
}
