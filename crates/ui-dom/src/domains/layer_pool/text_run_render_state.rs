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

use crate::domains::layer_ops::{FontFaceId, TextAlign, TextRunId, TextRunPaintOp, TextWrapMode};
use crate::domains::style::computed::TextFamilyList;
use crate::domains::text_layout::{self, FontStyleKw, FontWeightKw, TextFontCatalog};
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
    pub(super) font_catalog: TextFontCatalog,
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
    /// The glyph-coverage demotion count for the current surface apply
    /// (reset per `apply_text_runs_to_surface`; the executor folds it into
    /// `LayerPoolMetricName::TextRunGlyphCoverageDemotion`).
    pub(super) glyph_coverage_demotion_count: u64,
}

impl TextRunRenderState {
    pub(super) fn set_required_font_asset_ids(
        &mut self,
        face_id: FontFaceId,
        required_font_asset_ids: Vec<AssetId<UiDomFontAsset>>,
    ) {
        self.font_registration_state
            .set_required_font_asset_ids(face_id, required_font_asset_ids);
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
            if !self.font_registration_state.ready(request.key.font_face_id) {
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
                &self.font_catalog,
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
use super::{LayerConfig, LayerPool, LayerPoolMetricName, LayerPoolRenderFeedback};
#[cfg(test)]
use crate::domains::layer_ops::TextLanguageId;
#[cfg(test)]
use crate::domains::layer_pool::layer_pool_text_render::create_text_run_render_state;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::asset::Assets;
    use bevy::prelude::World;
    use bevy::render::render_resource::{Sampler, Texture};
    use bevy::render::renderer::{RenderDevice, RenderQueue, WgpuWrapper};
    use bevy::render::texture::GpuImage;
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
    fn render_state_constructs_pipelines_and_starts_without_ready_face() {
        let (_render_device, state) = fresh_text_run_render_state();
        assert!(state.run_bank.is_empty());
        assert!(state.pending_lease_dependencies.is_empty());
        assert!(
            !state
                .font_registration_state
                .ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS)
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

        state.set_required_font_asset_ids(FontFaceId::EMBEDDED_IBM_PLEX_SANS, vec![id_a, id_b]);
        assert!(
            !state
                .font_registration_state
                .ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS)
        );

        state.register_font_asset(ExtractedFontAssetRecord {
            asset_id: id_a,
            bytes: Arc::from(&b"a"[..]),
        });
        assert!(
            !state
                .font_registration_state
                .ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS)
        );

        state.register_font_asset(ExtractedFontAssetRecord {
            asset_id: id_b,
            bytes: Arc::from(&b"b"[..]),
        });
        assert!(
            state
                .font_registration_state
                .ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS)
        );
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

    fn lease_in_world() -> LayerLease {
        let mut world = World::new();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 10.0, 10.0))
    }

    fn text_run_op(text_run_id: TextRunId) -> TextRunPaintOp {
        TextRunPaintOp {
            text_run_id,
            position_px: [4.0, 6.0],
            size_px: [120.0, 40.0],
            font_size_px: 16.0,
            line_height_px: 20.0,
            wrap_mode: TextWrapMode::WordOrGlyph,
            alignment: TextAlign::Left,
            color_rgba: [1.0, 1.0, 1.0, 1.0],
        }
    }

    /// A `FontSystem` holding ONLY the catalog's faces (no system fonts), so a
    /// script the catalog does not cover shapes to glyph id 0 (notdef) — the
    /// demotion precondition "Thai text, no Thai face".
    fn catalog_only_font_system(catalog: &TextFontCatalog) -> FontSystem {
        let mut db = cosmic_text::fontdb::Database::new();
        for source in catalog.sources_snapshot() {
            db.load_font_source(source.font_db_source());
        }
        FontSystem::new_with_locale_and_db_and_fallback(
            "en-US".to_string(),
            db,
            cosmic_text::PlatformFallback,
        )
    }

    /// Folds the render-world demotion count into the `LayerPool` metric
    /// exactly as the executor's feedback flush does, and returns the counter.
    fn fold_demotion(state: &TextRunRenderState, pool: &mut LayerPool) -> u64 {
        let feedback = LayerPoolRenderFeedback {
            text_run_glyph_coverage_demotion: state.glyph_coverage_demotion_count,
            ..LayerPoolRenderFeedback::default()
        };
        feedback.apply_to(pool);
        pool.count(LayerPoolMetricName::TextRunGlyphCoverageDemotion)
    }

    #[test]
    fn glyph_coverage_demotion_counts() {
        let (render_device, render_queue) = noop_render_device_and_queue();
        let mut state = create_text_run_render_state(&render_device, PixelDensity::X1);
        // Keep the test focused on the demotion path, not the startup warmups,
        // and drop the system fonts so the catalog is the only face source
        // (establishing the "no Thai face" precondition).
        state.preload_warmup_queue = PreloadWarmupQueue::default();
        state.font_system = catalog_only_font_system(&state.font_catalog);
        let gpu_image = noop_gpu_image(&render_device);
        let lease = lease_in_world();

        // Mark the sans face ready, so both runs resolve and shape against it.
        let mut assets = Assets::<UiDomFontAsset>::default();
        let asset_id = assets
            .add(UiDomFontAsset {
                bytes: Arc::from(&b"x"[..]),
            })
            .id();
        state
            .font_registration_state
            .set_required_font_asset_ids(FontFaceId::EMBEDDED_IBM_PLEX_SANS, vec![asset_id]);
        state.font_registration_state.mark_registered(asset_id);
        assert!(
            state
                .font_registration_state
                .ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS)
        );

        let mut text_run_ids = SlotMap::with_key();
        let english_id = text_run_ids.insert(());
        let thai_id = text_run_ids.insert(());
        state.insert_run_record(
            english_id,
            RenderTextRunRecord {
                text: Arc::<str>::from("hello"),
                language_hint_id: TextLanguageId::English,
                family: TextFamilyList::default(),
                weight: FontWeightKw::NORMAL,
                style: FontStyleKw::Normal,
            },
        );
        state.insert_run_record(
            thai_id,
            RenderTextRunRecord {
                text: Arc::<str>::from("เมนู"),
                language_hint_id: TextLanguageId::Thai,
                family: TextFamilyList::default(),
                weight: FontWeightKw::NORMAL,
                style: FontStyleKw::Normal,
            },
        );

        let surface_context = TextRunSurfaceContext {
            device: &render_device,
            queue: &render_queue,
            gpu_image: &gpu_image,
            layer_zero_in_image_px: [0.0, 0.0],
            lease,
        };

        // Covered side: English shapes with real glyphs — no demotion, so the
        // counter stays at zero.
        assert!(
            state.apply_to_surface(&surface_context, &[text_run_op(english_id)]),
            "the covered run should apply without pending dependencies",
        );
        let mut pool = LayerPool::new(LayerConfig::default());
        assert_eq!(fold_demotion(&state, &mut pool), 0);

        // Uncovered side: Thai has no Thai face — glyph id 0 (notdef) demotes
        // the run to a debug rect, which increments the counter.
        assert!(
            state.apply_to_surface(&surface_context, &[text_run_op(thai_id)]),
            "the uncovered run should apply without pending dependencies",
        );
        assert_eq!(fold_demotion(&state, &mut pool), 1);
    }
}
